//! Take an offer — build the taker's unsigned spends, then combine them with the maker's half.
//!
//! Taking is a two-phase, one-context flow that keeps the taker's key on the caller's side:
//!
//! 1. [`take_build`] decodes the offer and, in the SAME [`SpendContext`], claims the maker's
//!    offered coins to the taker and funds the maker's requested payments, returning the taker's
//!    UNSIGNED coin spends plus the parsed maker [`Offer`].
//! 2. The caller signs the taker's coin spends and wraps them in a `SpendBundle`.
//! 3. [`take_combine`] concatenates the maker's already-signed half with the signed taker half
//!    into one atomic settlement bundle for the caller to broadcast.
//!
//! Decode and build share one context because a parsed offer's offered-NFT metadata is an
//! allocator-relative pointer — reconstructing the offered assets must use that same allocator.

use chia_protocol::{Bytes32, SpendBundle};
use chia_wallet_sdk::driver::{Action, Offer, Relation, SpendContext, Spends};
use chia_wallet_sdk::prelude::PublicKey;
use indexmap::IndexMap;

use crate::error::{Error, Result};
use crate::hydrate::{decode, parse};
use crate::types::{OfferCost, TakerFunds, UnsignedTake};

/// Build the taker's UNSIGNED spends for taking the offer encoded by `offer_str`, funding the
/// requested payments from `funds` and reserving `fee` (mojos).
///
/// The maker's offered coins are claimed to `funds.change_puzzle_hash`; the requested payments are
/// funded from the taker's coins; change and received assets return to the change address. Returns
/// the taker's unsigned coin spends, the parsed maker [`Offer`] (its half already signed), and the
/// cost the take funds — pass the first two to [`take_combine`] after signing.
///
/// Errors before building if `offer_str` is not a valid offer ([`Error::Decode`]) or the taker's
/// funds cannot cover the requested cost ([`Error::InvalidInput`], with the shortfall named).
pub fn take_build(
    ctx: &mut SpendContext,
    offer_str: &str,
    funds: TakerFunds<'_>,
    fee: u64,
) -> Result<UnsignedTake> {
    let spend_bundle = decode(offer_str)?;
    let offer = parse(ctx, &spend_bundle)?;

    let cost = arbitrage_cost(&offer);
    ensure_funds_cover(&funds, &cost)?;

    let mut spends = Spends::new(funds.change_puzzle_hash);
    spends.add(offer.offered_coins().clone());
    let key_map: IndexMap<Bytes32, PublicKey> = funds.owner_keys.clone();
    for coin in &funds.xch_coins {
        spends.add(*coin);
    }
    for cat in &funds.cat_coins {
        spends.add(*cat);
    }
    for nft in &funds.nfts {
        spends.add(*nft);
    }

    let mut actions = offer.requested_payments().actions();
    if fee > 0 {
        actions.push(Action::fee(fee));
    }

    let deltas = spends.apply(ctx, &actions)?;
    spends.finish_with_keys(ctx, &deltas, Relation::AssertConcurrent, &key_map)?;

    Ok(UnsignedTake {
        coin_spends: ctx.take(),
        offer,
        cost,
    })
}

/// Combine the maker's already-signed `offer` with the taker's `signed_taker` bundle into one
/// atomic settlement `SpendBundle` ready to broadcast.
///
/// This is a pure concatenation of coin spends and aggregation of signatures — it is
/// allocator-free, so the returned bundle is safe to hand out beyond any build context.
#[must_use]
pub fn take_combine(offer: Offer, signed_taker: SpendBundle) -> SpendBundle {
    offer.take(signed_taker)
}

/// The taker's cost: the requested-over-offered surplus (the offer's arbitrage offered side).
fn arbitrage_cost(offer: &Offer) -> OfferCost {
    let arbitrage = offer.arbitrage();
    OfferCost {
        xch: arbitrage.offered.xch,
        cats: arbitrage
            .offered
            .cats
            .iter()
            .map(|(asset_id, amount)| (*asset_id, *amount))
            .collect(),
    }
}

/// Verify the taker's funds cover the fungible cost, naming any shortfall before a spend is built.
fn ensure_funds_cover(funds: &TakerFunds<'_>, cost: &OfferCost) -> Result<()> {
    let available_xch: u64 = funds.xch_coins.iter().map(|coin| coin.amount).sum();
    if available_xch < cost.xch {
        return Err(Error::invalid(format!(
            "insufficient XCH to take: need {}, have {available_xch}",
            cost.xch
        )));
    }
    for (asset_id, need) in &cost.cats {
        let available: u64 = funds
            .cat_coins
            .iter()
            .filter(|cat| cat.info.asset_id == *asset_id)
            .map(|cat| cat.coin.amount)
            .sum();
        if available < *need {
            return Err(Error::invalid(format!(
                "insufficient CAT {asset_id} to take: need {need}, have {available}"
            )));
        }
    }
    Ok(())
}
