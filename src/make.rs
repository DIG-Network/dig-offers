//! Build the maker's side of an offer — the unsigned spends, then the assembled `offer1…` string.
//!
//! Making an offer is a two-phase, one-context flow that keeps the secret key on the caller's
//! side:
//!
//! 1. [`make_build`] spends the maker's offered coins into the settlement puzzle and *asserts*
//!    the requested payments (never funding them), returning the UNSIGNED coin spends plus the
//!    requested-payment context.
//! 2. The caller computes the required signatures ([`required_signatures`](crate::required_signatures)),
//!    signs, and wraps the result in a `SpendBundle`.
//! 3. [`make_assemble`] — using the SAME [`SpendContext`] as step 1 — folds the signed maker
//!    bundle and the requested payments into a one-sided offer and encodes it.
//!
//! The requested side is an ASSERTION plus a phantom carrier, not a settle action: the maker must
//! never self-fund both sides of its own offer.

use chia_protocol::{Bytes32, Coin, SpendBundle};
use chia_puzzle_types::offer::{NotarizedPayment, Payment};
use chia_puzzle_types::Memos;
use chia_wallet_sdk::driver::{
    encode_offer, Action, AssetInfo, Cat, CatAssetInfo, Id, Offer, Relation, RequestedPayments,
    SpendContext, Spends,
};
use chia_wallet_sdk::prelude::PublicKey;
use chia_wallet_sdk::types::puzzles::SettlementPayment;
use chia_wallet_sdk::types::Mod;
use indexmap::IndexMap;

use crate::error::{Error, Result};
use crate::types::{OfferedSide, RequestedSide, UnsignedMake};

/// The offer settlement puzzle hash (`SETTLEMENT_PAYMENT_HASH`), sourced from the settlement
/// mod's own hash so the crate needs no direct dependency on the puzzle-constants crate.
fn settlement_payment_hash() -> Bytes32 {
    Bytes32::from(<[u8; 32]>::from(SettlementPayment::mod_hash()))
}

/// Build the maker's UNSIGNED offer spends: spend `offered` into settlement, assert `requested`,
/// and reserve `fee` (mojos).
///
/// The offered coins are selected from `offered`'s funding coins (largest first) to cover the
/// offered amounts plus the fee; surplus returns to `offered.change_puzzle_hash`. The requested
/// payments are notarized to a nonce derived from the exact offered coins and asserted — the
/// maker does not fund them. Returns the unsigned coin spends and the requested-payment context
/// to pass to [`make_assemble`] after signing (in the SAME `ctx`).
///
/// Errors ([`Error::InvalidInput`]) if neither side has any asset, a requested amount is zero, or
/// the funding coins cannot cover the offered amounts (with the shortfall named).
pub fn make_build(
    ctx: &mut SpendContext,
    offered: OfferedSide<'_>,
    requested: RequestedSide,
    fee: u64,
) -> Result<UnsignedMake> {
    ensure_non_empty(&offered, &requested)?;

    let settlement_ph = settlement_payment_hash();
    let mut spends = Spends::new(offered.change_puzzle_hash);
    let key_map: IndexMap<Bytes32, PublicKey> = offered.owner_keys.clone();
    let mut actions: Vec<Action> = Vec::new();
    let mut offered_coin_ids: Vec<Bytes32> = Vec::new();

    add_offered_nfts(
        &offered,
        &mut spends,
        &mut actions,
        &mut offered_coin_ids,
        settlement_ph,
    );
    add_offered_xch(
        &offered,
        fee,
        &mut spends,
        &mut actions,
        &mut offered_coin_ids,
        settlement_ph,
    )?;
    add_offered_cats(
        &offered,
        &mut spends,
        &mut actions,
        &mut offered_coin_ids,
        settlement_ph,
    )?;

    if offered_coin_ids.is_empty() {
        return Err(Error::invalid("make selected no offered coins to spend"));
    }

    let nonce = Offer::nonce(offered_coin_ids);
    let (requested_payments, requested_asset_info) = build_requested(ctx, &requested, nonce)?;

    if fee > 0 {
        actions.push(Action::fee(fee));
    }

    let deltas = spends.apply(ctx, &actions)?;
    spends.conditions.required = spends
        .conditions
        .required
        .extend(requested_payments.assertions(ctx, &requested_asset_info)?);
    spends.finish_with_keys(ctx, &deltas, Relation::AssertConcurrent, &key_map)?;

    Ok(UnsignedMake {
        coin_spends: ctx.take(),
        requested_payments,
        requested_asset_info,
        nonce,
    })
}

/// Assemble a signed maker bundle into a one-sided `offer1…` string.
///
/// `signed` is the `SpendBundle` the caller produced by signing [`make_build`]'s coin spends;
/// `requested_payments` and `requested_asset_info` are the values [`make_build`] returned. This
/// MUST run in the SAME `ctx` as the matching [`make_build`], because a requested-NFT leg carries
/// an allocator-relative metadata pointer that only survives in that context.
pub fn make_assemble(
    ctx: &mut SpendContext,
    signed: SpendBundle,
    requested_payments: RequestedPayments,
    requested_asset_info: AssetInfo,
) -> Result<String> {
    let offer =
        Offer::from_input_spend_bundle(ctx, signed, requested_payments, requested_asset_info)?;
    let spend_bundle = offer.to_spend_bundle(ctx)?;
    encode_offer(&spend_bundle).map_err(|e| Error::decode(format!("could not encode offer: {e}")))
}

/// Reject a make with no offered asset or no requested asset — an offer must have both sides.
fn ensure_non_empty(offered: &OfferedSide<'_>, requested: &RequestedSide) -> Result<()> {
    let offers_nothing =
        offered.offer_xch == 0 && offered.offer_cats.is_empty() && offered.nfts.is_empty();
    if offers_nothing {
        return Err(Error::invalid("make must offer at least one asset"));
    }
    let requests_nothing =
        requested.xch == 0 && requested.cats.is_empty() && requested.nfts.is_empty();
    if requests_nothing {
        return Err(Error::invalid("make must request at least one asset"));
    }
    Ok(())
}

/// Spend each offered NFT whole into settlement.
fn add_offered_nfts(
    offered: &OfferedSide<'_>,
    spends: &mut Spends,
    actions: &mut Vec<Action>,
    offered_coin_ids: &mut Vec<Bytes32>,
    settlement_ph: Bytes32,
) {
    for nft in &offered.nfts {
        spends.add(*nft);
        offered_coin_ids.push(nft.coin.coin_id());
        actions.push(Action::send(
            Id::Existing(nft.info.launcher_id),
            settlement_ph,
            1,
            Memos::None,
        ));
    }
}

/// Select XCH coins covering the offered XCH plus the fee, and (when XCH is offered) spend it into
/// settlement. Change returns to the maker's change address automatically.
fn add_offered_xch(
    offered: &OfferedSide<'_>,
    fee: u64,
    spends: &mut Spends,
    actions: &mut Vec<Action>,
    offered_coin_ids: &mut Vec<Bytes32>,
    settlement_ph: Bytes32,
) -> Result<()> {
    let xch_needed = offered.offer_xch.saturating_add(fee);
    if xch_needed > 0 {
        for coin in select_xch(&offered.xch_coins, xch_needed)? {
            spends.add(coin);
            offered_coin_ids.push(coin.coin_id());
        }
    }
    if offered.offer_xch > 0 {
        actions.push(Action::send(
            Id::Xch,
            settlement_ph,
            offered.offer_xch,
            Memos::None,
        ));
    }
    Ok(())
}

/// Select CAT coins covering each offered CAT leg and spend it into settlement.
fn add_offered_cats(
    offered: &OfferedSide<'_>,
    spends: &mut Spends,
    actions: &mut Vec<Action>,
    offered_coin_ids: &mut Vec<Bytes32>,
    settlement_ph: Bytes32,
) -> Result<()> {
    for (asset_id, amount) in &offered.offer_cats {
        for cat in select_cats(&offered.cat_coins, *asset_id, *amount)? {
            spends.add(cat);
            offered_coin_ids.push(cat.coin.coin_id());
        }
        actions.push(Action::send(
            Id::Existing(*asset_id),
            settlement_ph,
            *amount,
            Memos::None,
        ));
    }
    Ok(())
}

/// Build the notarized, asserted requested payments and their asset info from `requested`.
fn build_requested(
    ctx: &mut SpendContext,
    requested: &RequestedSide,
    nonce: Bytes32,
) -> Result<(RequestedPayments, AssetInfo)> {
    let payee = requested.payee_puzzle_hash;
    let hint = ctx.hint(payee)?;
    let mut payments = RequestedPayments::new();
    let mut asset_info = AssetInfo::new();

    if requested.xch > 0 {
        payments.xch.push(NotarizedPayment::new(
            nonce,
            vec![Payment::new(payee, requested.xch, hint)],
        ));
    }
    for (asset_id, amount) in &requested.cats {
        if *amount == 0 {
            return Err(Error::invalid(
                "requested CAT amount must be greater than zero",
            ));
        }
        payments.cats.insert(
            *asset_id,
            vec![NotarizedPayment::new(
                nonce,
                vec![Payment::new(payee, *amount, hint)],
            )],
        );
        asset_info.insert_cat(*asset_id, CatAssetInfo::new(None))?;
    }
    for (launcher_id, nft_info) in &requested.nfts {
        payments.nfts.insert(
            *launcher_id,
            vec![NotarizedPayment::new(
                nonce,
                vec![Payment::new(payee, 1, hint)],
            )],
        );
        asset_info.insert_nft(*launcher_id, *nft_info)?;
    }
    Ok((payments, asset_info))
}

/// Greedily select XCH coins (largest first) covering `need`, returning the chosen coins.
fn select_xch(coins: &[Coin], need: u64) -> Result<Vec<Coin>> {
    let mut sorted: Vec<Coin> = coins.to_vec();
    sorted.sort_by_key(|coin| std::cmp::Reverse(coin.amount));
    let mut sum = 0u64;
    let mut chosen = Vec::new();
    for coin in sorted {
        if sum >= need {
            break;
        }
        sum = sum.saturating_add(coin.amount);
        chosen.push(coin);
    }
    if sum < need {
        return Err(Error::invalid(format!(
            "insufficient XCH to offer: need {need}, have {sum}"
        )));
    }
    Ok(chosen)
}

/// Greedily select CAT coins of `asset_id` (largest first) covering `need`, returning them.
fn select_cats(coins: &[Cat], asset_id: Bytes32, need: u64) -> Result<Vec<Cat>> {
    let mut sorted: Vec<Cat> = coins
        .iter()
        .filter(|cat| cat.info.asset_id == asset_id)
        .copied()
        .collect();
    sorted.sort_by_key(|cat| std::cmp::Reverse(cat.coin.amount));
    let mut sum = 0u64;
    let mut chosen = Vec::new();
    for cat in sorted {
        if sum >= need {
            break;
        }
        sum = sum.saturating_add(cat.coin.amount);
        chosen.push(cat);
    }
    if sum < need {
        return Err(Error::invalid(format!(
            "insufficient CAT {asset_id} to offer: need {need}, have {sum}"
        )));
    }
    Ok(chosen)
}

#[cfg(test)]
mod tests {
    use std::marker::PhantomData;

    use super::*;
    use chia_sdk_test::Simulator;

    use crate::test_support::owner_keys;
    use crate::types::{OfferedSide, RequestedSide};

    /// An offered side that offers nothing (used for negative tests).
    fn empty_offered<'a>(change: Bytes32) -> OfferedSide<'a> {
        OfferedSide {
            change_puzzle_hash: change,
            owner_keys: IndexMap::new(),
            xch_coins: vec![],
            cat_coins: vec![],
            nfts: vec![],
            offer_xch: 0,
            offer_cats: vec![],
            _pd: PhantomData,
        }
    }

    #[test]
    fn make_build_rejects_offering_nothing() {
        let mut ctx = SpendContext::new();
        let requested = RequestedSide {
            payee_puzzle_hash: Bytes32::default(),
            xch: 1,
            cats: vec![],
            nfts: vec![],
        };
        let err =
            make_build(&mut ctx, empty_offered(Bytes32::default()), requested, 0).unwrap_err();
        assert!(matches!(&err, Error::InvalidInput(m) if m.contains("offer at least one")));
    }

    #[test]
    fn make_build_rejects_requesting_nothing() {
        let mut ctx = SpendContext::new();
        let mut offered = empty_offered(Bytes32::default());
        offered.offer_xch = 1;
        let requested = RequestedSide {
            payee_puzzle_hash: Bytes32::default(),
            xch: 0,
            cats: vec![],
            nfts: vec![],
        };
        let err = make_build(&mut ctx, offered, requested, 0).unwrap_err();
        assert!(matches!(&err, Error::InvalidInput(m) if m.contains("request at least one")));
    }

    #[test]
    fn make_build_rejects_insufficient_offered_funds() {
        let mut ctx = SpendContext::new();
        let mut offered = empty_offered(Bytes32::default());
        offered.offer_xch = 1_000; // no XCH coins provided → shortfall
        let requested = RequestedSide {
            payee_puzzle_hash: Bytes32::default(),
            xch: 1,
            cats: vec![],
            nfts: vec![],
        };
        let err = make_build(&mut ctx, offered, requested, 0).unwrap_err();
        assert!(matches!(&err, Error::InvalidInput(m) if m.contains("insufficient XCH to offer")));
    }

    #[test]
    fn make_build_rejects_zero_requested_cat_amount() {
        let mut sim = Simulator::new();
        let mut ctx = SpendContext::new();
        let maker = sim.bls(1_000_000);

        let offered = OfferedSide {
            change_puzzle_hash: maker.puzzle_hash,
            owner_keys: owner_keys(&maker),
            xch_coins: vec![maker.coin],
            cat_coins: vec![],
            nfts: vec![],
            offer_xch: 100_000,
            offer_cats: vec![],
            _pd: PhantomData,
        };
        let requested = RequestedSide {
            payee_puzzle_hash: maker.puzzle_hash,
            xch: 0,
            cats: vec![(Bytes32::from([7; 32]), 0)],
            nfts: vec![],
        };
        let err = make_build(&mut ctx, offered, requested, 0).unwrap_err();
        assert!(matches!(&err, Error::InvalidInput(m) if m.contains("greater than zero")));
    }
}
