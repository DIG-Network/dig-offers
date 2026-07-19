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
