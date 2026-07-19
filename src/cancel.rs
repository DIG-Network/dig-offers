//! Cancel an offer the caller made — reclaim its offered coins, invalidating the outstanding
//! `offer1…` string.
//!
//! An offered coin is spent into settlement inside the offer bundle, but that spend only settles
//! when a taker fulfils it. To cancel, the maker spends the SAME (still-unspent) coins to a
//! reclaim address instead — a competing spend that makes the offer un-takeable. [`cancel_build`]
//! returns those reclaim spends UNSIGNED; the caller signs and broadcasts.
//!
//! Cancellation re-spends each offered coin through its standard (p2) layer, so it supports the
//! standard-layer (XCH) offered coins whose authorizing public key the caller supplies. A CAT- or
//! NFT-offered coin must be reclaimed through its own native layer and is reported as unsupported.

use chia_protocol::Bytes32;
use chia_puzzle_types::Memos;
use chia_wallet_sdk::driver::{SpendContext, StandardLayer};
use chia_wallet_sdk::prelude::PublicKey;
use chia_wallet_sdk::types::Conditions;
use indexmap::IndexMap;

use crate::error::{Error, Result};
use crate::hydrate::{decode, parse};
use crate::types::UnsignedCancel;

/// Build the UNSIGNED spends that reclaim the offered coins of `offer_str` to
/// `reclaim_puzzle_hash`, reserving `fee` (mojos) on the first spend.
///
/// `owner_keys` maps each offered coin's puzzle hash to the public key authorizing it. Returns the
/// unsigned reclaim coin spends for the caller to sign.
///
/// Errors ([`Error::InvalidInput`]) if the offer has no cancellable coins, or if an offered coin
/// is not a standard-layer coin the caller has a key for (a CAT/NFT offered coin must be reclaimed
/// through its native layer, which this builder does not construct).
pub fn cancel_build(
    ctx: &mut SpendContext,
    offer_str: &str,
    reclaim_puzzle_hash: Bytes32,
    owner_keys: &IndexMap<Bytes32, PublicKey>,
    fee: u64,
) -> Result<UnsignedCancel> {
    let spend_bundle = decode(offer_str)?;
    let offer = parse(ctx, &spend_bundle)?;

    let cancellable = offer.cancellable_coin_spends()?;
    if cancellable.is_empty() {
        return Err(Error::invalid(
            "no cancellable coins in this offer (already settled, or not the maker's)",
        ));
    }

    let mut fee_pending = fee;
    for coin_spend in &cancellable {
        let coin = coin_spend.coin;
        let public_key = owner_keys.get(&coin.puzzle_hash).ok_or_else(|| {
            Error::invalid(format!(
                "no key for offered coin {} (cancel supports standard-layer (XCH) offered coins; \
                 reclaim a CAT/NFT offered coin through its native layer)",
                coin.puzzle_hash
            ))
        })?;

        let mut conditions =
            Conditions::new().create_coin(reclaim_puzzle_hash, coin.amount, Memos::None);
        if fee_pending > 0 {
            conditions = conditions.reserve_fee(fee_pending);
            fee_pending = 0;
        }
        StandardLayer::new(*public_key).spend(ctx, coin, conditions)?;
    }

    Ok(UnsignedCancel {
        coin_spends: ctx.take(),
    })
}

#[cfg(test)]
mod tests {
    use std::marker::PhantomData;

    use super::*;
    use chia_sdk_test::Simulator;

    use crate::test_support::{
        issue_cat_to, owner_keys, sign_for_sim, signed_bundle, taker_settle,
    };
    use crate::types::{OfferedSide, RequestedSide, TakerFunds};
    use crate::{make_assemble, make_build};

    /// Build an offer that offers all of `maker`'s coin as XCH, requesting `amount` of the CAT
    /// `asset` paid to the maker.
    fn make_xch_offer(
        sim: &mut Simulator,
        ctx: &mut SpendContext,
        maker: &chia_sdk_test::BlsPairWithCoin,
        asset: Bytes32,
        amount: u64,
    ) -> anyhow::Result<String> {
        let _ = sim;
        let offered = OfferedSide {
            change_puzzle_hash: maker.puzzle_hash,
            owner_keys: owner_keys(maker),
            xch_coins: vec![maker.coin],
            cat_coins: vec![],
            nfts: vec![],
            offer_xch: maker.coin.amount,
            offer_cats: vec![],
            _pd: PhantomData,
        };
        let requested = RequestedSide {
            payee_puzzle_hash: maker.puzzle_hash,
            xch: 0,
            cats: vec![(asset, amount)],
            nfts: vec![],
        };
        let unsigned = make_build(ctx, offered, requested, 0)?;
        let signature = sign_for_sim(&unsigned.coin_spends, std::slice::from_ref(&maker.sk))?;
        Ok(make_assemble(
            ctx,
            signed_bundle(unsigned.coin_spends, signature),
            unsigned.requested_payments,
            unsigned.requested_asset_info,
        )?)
    }

    #[test]
    fn cancel_reclaims_offered_coins_and_invalidates_the_offer() -> anyhow::Result<()> {
        let mut sim = Simulator::new();
        let mut ctx = SpendContext::new();
        let maker = sim.bls(1_000_000);
        let taker = sim.bls(0);
        let (taker_cat, asset) = issue_cat_to(&mut sim, &mut ctx, &taker, 30_000)?;
        let offer_str = make_xch_offer(&mut sim, &mut ctx, &maker, asset, 30_000)?;

        let unsigned = cancel_build(
            &mut ctx,
            &offer_str,
            maker.puzzle_hash,
            &owner_keys(&maker),
            0,
        )?;
        assert!(!unsigned.coin_spends.is_empty());

        let signature = sign_for_sim(&unsigned.coin_spends, std::slice::from_ref(&maker.sk))?;
        sim.new_transaction(signed_bundle(unsigned.coin_spends, signature))?;

        // The offered coin is reclaimed to the maker; the outstanding offer can no longer settle.
        assert_eq!(
            sim.unspent_coins(maker.puzzle_hash, false)
                .iter()
                .map(|c| c.amount)
                .sum::<u64>(),
            1_000_000,
            "the full offered coin is reclaimed to the maker"
        );

        let funds = TakerFunds {
            change_puzzle_hash: taker.puzzle_hash,
            owner_keys: owner_keys(&taker),
            xch_coins: vec![],
            cat_coins: vec![taker_cat],
            nfts: vec![],
            _pd: PhantomData,
        };
        let bundle = taker_settle(
            &mut ctx,
            &offer_str,
            funds,
            0,
            std::slice::from_ref(&taker.sk),
        )?;
        assert!(
            sim.new_transaction(bundle).is_err(),
            "a cancelled offer must no longer settle (its offered coin is spent)"
        );
        Ok(())
    }

    #[test]
    fn cancel_rejects_non_offer() {
        let mut ctx = SpendContext::new();
        let err = cancel_build(
            &mut ctx,
            "not an offer",
            Bytes32::default(),
            &IndexMap::new(),
            0,
        )
        .unwrap_err();
        assert!(matches!(&err, Error::Decode(_)));
    }

    #[test]
    fn cancel_rejects_offered_coin_without_a_key() -> anyhow::Result<()> {
        let mut sim = Simulator::new();
        let mut ctx = SpendContext::new();
        let maker = sim.bls(1_000_000);
        let offer_str = make_xch_offer(&mut sim, &mut ctx, &maker, Bytes32::from([9; 32]), 30_000)?;

        // No key supplied for the offered coin → refuse rather than build an unsignable spend.
        let err =
            cancel_build(&mut ctx, &offer_str, maker.puzzle_hash, &IndexMap::new(), 0).unwrap_err();
        assert!(matches!(&err, Error::InvalidInput(m) if m.contains("no key for offered coin")));
        Ok(())
    }
}
