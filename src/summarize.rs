//! Inspect an `offer1…` string without committing to it.
//!
//! [`summarize`] decodes an offer and reports what it offers, what it requests, the taker's
//! arbitrage cost, and any NFT royalties — a read-only view for a wallet's "review this offer"
//! surface. It builds nothing and signs nothing.

use chia_wallet_sdk::driver::{Offer, SpendContext};

use crate::error::Result;
use crate::hydrate::{decode, parse};
use crate::types::{OfferAsset, OfferCost, OfferSummary};

/// Summarize the offer encoded by `offer_str`: its offered and requested assets, the taker's
/// arbitrage cost, and its royalties.
///
/// Errors if `offer_str` is not a valid offer. The summary reflects exactly what the canonical
/// [`Offer`] carries, so it never over- or under-states a leg.
pub fn summarize(offer_str: &str) -> Result<OfferSummary> {
    let spend_bundle = decode(offer_str)?;
    let mut ctx = SpendContext::new();
    let offer = parse(&mut ctx, &spend_bundle)?;

    Ok(OfferSummary {
        offered: offered_assets(&offer),
        requested: requested_assets(&offer),
        arbitrage: arbitrage_cost(&offer),
        royalties: royalties(&offer),
    })
}

/// The assets the offer delivers to the taker (offered XCH, CATs, and NFTs).
fn offered_assets(offer: &Offer) -> Vec<OfferAsset> {
    let coins = offer.offered_coins();
    let amounts = coins.amounts();
    let mut assets = fungible_assets(amounts.xch, amounts.cats.iter().map(|(id, a)| (*id, *a)));
    assets.extend(coins.nfts.keys().map(|launcher_id| OfferAsset::Nft {
        launcher_id: *launcher_id,
    }));
    assets
}

/// The assets the offer asks the taker to pay (requested XCH, CATs, and NFTs).
fn requested_assets(offer: &Offer) -> Vec<OfferAsset> {
    let payments = offer.requested_payments();
    let amounts = payments.amounts();
    let mut assets = fungible_assets(amounts.xch, amounts.cats.iter().map(|(id, a)| (*id, *a)));
    assets.extend(payments.nfts.keys().map(|launcher_id| OfferAsset::Nft {
        launcher_id: *launcher_id,
    }));
    assets
}

/// Assemble the fungible legs (XCH first, then each CAT) into a leg list, omitting zero amounts.
fn fungible_assets(
    xch: u64,
    cats: impl Iterator<Item = (chia_protocol::Bytes32, u64)>,
) -> Vec<OfferAsset> {
    let mut assets = Vec::new();
    if xch > 0 {
        assets.push(OfferAsset::Xch(xch));
    }
    assets.extend(cats.map(|(asset_id, amount)| OfferAsset::Cat { asset_id, amount }));
    assets
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

/// Every royalty leg the offer carries (offered- and requested-side NFT royalties).
fn royalties(offer: &Offer) -> Vec<(chia_protocol::Bytes32, u16)> {
    offer
        .offered_royalties()
        .into_iter()
        .chain(offer.requested_royalties())
        .map(|royalty| (royalty.launcher_id, royalty.basis_points))
        .collect()
}

#[cfg(test)]
mod tests {
    use std::marker::PhantomData;

    use chia_sdk_test::Simulator;
    use chia_wallet_sdk::driver::SpendContext;

    use crate::test_support::{maker_offer, mint_nft_for, owner_keys, sample_cat_for_xch};
    use crate::types::{OfferedSide, RequestedSide};
    use crate::{summarize, Error, OfferAsset};

    #[test]
    fn summarize_rejects_non_offer() {
        assert!(matches!(summarize("not an offer"), Err(Error::Decode(_))));
        assert!(matches!(summarize("   "), Err(Error::Decode(_))));
    }

    #[test]
    fn summarize_reports_both_sides_and_arbitrage() -> anyhow::Result<()> {
        let mut sim = Simulator::new();
        let mut ctx = SpendContext::new();
        let (offer_str, _maker, asset) = sample_cat_for_xch(&mut sim, &mut ctx, 80_000, 50_000)?;

        let summary = summarize(&offer_str)?;
        assert_eq!(
            summary.offered,
            vec![OfferAsset::Cat {
                asset_id: asset,
                amount: 80_000
            }]
        );
        assert_eq!(summary.requested, vec![OfferAsset::Xch(50_000)]);
        assert_eq!(summary.arbitrage.xch, 50_000);
        assert!(summary.royalties.is_empty());
        Ok(())
    }

    #[test]
    fn summarize_reports_offered_nft_royalty() -> anyhow::Result<()> {
        let mut sim = Simulator::new();
        let mut ctx = SpendContext::new();
        let maker = sim.bls(2);
        let nft = mint_nft_for(&mut sim, &mut ctx, &maker, 300)?;

        let offered = OfferedSide {
            change_puzzle_hash: maker.puzzle_hash,
            owner_keys: owner_keys(&maker),
            xch_coins: vec![],
            cat_coins: vec![],
            nfts: vec![nft],
            offer_xch: 0,
            offer_cats: vec![],
            _pd: PhantomData,
        };
        let requested = RequestedSide {
            payee_puzzle_hash: maker.puzzle_hash,
            xch: 100_000,
            cats: vec![],
            nfts: vec![],
        };
        let offer_str = maker_offer(
            &mut ctx,
            offered,
            requested,
            0,
            std::slice::from_ref(&maker.sk),
        )?;

        let summary = summarize(&offer_str)?;
        assert_eq!(
            summary.royalties,
            vec![(nft.info.launcher_id, 300)],
            "an offered royalty NFT's royalty must be reported"
        );
        Ok(())
    }
}
