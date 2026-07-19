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

#[cfg(test)]
mod tests {
    use std::marker::PhantomData;

    use super::*;
    use chia_sdk_test::Simulator;
    use dig_cat::cat_puzzle_hash;

    use crate::test_support::{issue_cat_to, maker_offer, mint_nft_for, owner_keys, taker_settle};
    use crate::types::{OfferedSide, RequestedSide, TakerFunds};
    use crate::{summarize, OfferAsset};

    /// Sum the amounts of every unspent coin sitting directly at `puzzle_hash`.
    fn balance_at(sim: &Simulator, puzzle_hash: Bytes32) -> u64 {
        sim.unspent_coins(puzzle_hash, false)
            .iter()
            .map(|coin| coin.amount)
            .sum()
    }

    #[test]
    fn take_cat_for_xch_settles_both_sides() -> anyhow::Result<()> {
        let mut sim = Simulator::new();
        let mut ctx = SpendContext::new();

        let maker = sim.bls(0);
        let taker = sim.bls(1_000_000);
        let offered_cat: u64 = 80_000;
        let requested_xch: u64 = 50_000;

        let (maker_cat, asset) = issue_cat_to(&mut sim, &mut ctx, &maker, offered_cat)?;

        let offered = OfferedSide {
            change_puzzle_hash: maker.puzzle_hash,
            owner_keys: owner_keys(&maker),
            xch_coins: vec![],
            cat_coins: vec![maker_cat],
            nfts: vec![],
            offer_xch: 0,
            offer_cats: vec![(asset, offered_cat)],
            _pd: PhantomData,
        };
        let requested = RequestedSide {
            payee_puzzle_hash: maker.puzzle_hash,
            xch: requested_xch,
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
            summary.offered,
            vec![OfferAsset::Cat {
                asset_id: asset,
                amount: offered_cat
            }]
        );
        assert_eq!(summary.requested, vec![OfferAsset::Xch(requested_xch)]);
        assert_eq!(summary.arbitrage.xch, requested_xch);

        let funds = TakerFunds {
            change_puzzle_hash: taker.puzzle_hash,
            owner_keys: owner_keys(&taker),
            xch_coins: vec![taker.coin],
            cat_coins: vec![],
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
        sim.new_transaction(bundle)?;

        assert_eq!(
            balance_at(&sim, maker.puzzle_hash),
            requested_xch,
            "maker must receive the requested XCH"
        );
        assert_eq!(
            balance_at(&sim, cat_puzzle_hash(taker.puzzle_hash, asset)),
            offered_cat,
            "taker must receive the offered CAT"
        );
        Ok(())
    }

    #[test]
    fn take_xch_for_cat_settles_both_sides_and_keeps_change() -> anyhow::Result<()> {
        let mut sim = Simulator::new();
        let mut ctx = SpendContext::new();

        let maker = sim.bls(1_000_000);
        let taker = sim.bls(0);
        let offered_xch: u64 = 200_000;
        let requested_cat: u64 = 30_000;

        let (taker_cat, asset) = issue_cat_to(&mut sim, &mut ctx, &taker, requested_cat)?;

        let offered = OfferedSide {
            change_puzzle_hash: maker.puzzle_hash,
            owner_keys: owner_keys(&maker),
            xch_coins: vec![maker.coin],
            cat_coins: vec![],
            nfts: vec![],
            offer_xch: offered_xch,
            offer_cats: vec![],
            _pd: PhantomData,
        };
        let requested = RequestedSide {
            payee_puzzle_hash: maker.puzzle_hash,
            xch: 0,
            cats: vec![(asset, requested_cat)],
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
        assert_eq!(summary.offered, vec![OfferAsset::Xch(offered_xch)]);
        assert_eq!(
            summary.requested,
            vec![OfferAsset::Cat {
                asset_id: asset,
                amount: requested_cat
            }]
        );
        assert_eq!(summary.arbitrage.cats, vec![(asset, requested_cat)]);

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
        sim.new_transaction(bundle)?;

        // The taker receives the offered XCH; the maker keeps the 800_000 change (no self-fund
        // drain) AND receives the requested CAT.
        assert_eq!(
            balance_at(&sim, taker.puzzle_hash),
            offered_xch,
            "taker must receive the offered XCH"
        );
        assert_eq!(
            balance_at(&sim, maker.puzzle_hash),
            1_000_000 - offered_xch,
            "maker must keep full change on its offered coin (no self-fund)"
        );
        assert_eq!(
            balance_at(&sim, cat_puzzle_hash(maker.puzzle_hash, asset)),
            requested_cat,
            "maker must receive the requested CAT"
        );
        Ok(())
    }

    #[test]
    fn take_nft_for_cat_settles_and_delivers_the_nft() -> anyhow::Result<()> {
        let mut sim = Simulator::new();
        let mut ctx = SpendContext::new();

        let maker = sim.bls(2);
        let taker = sim.bls(0);
        let price: u64 = 100_000;

        let (taker_cat, asset) = issue_cat_to(&mut sim, &mut ctx, &taker, price)?;
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
            xch: 0,
            cats: vec![(asset, price)],
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
            summary.offered,
            vec![OfferAsset::Nft {
                launcher_id: nft.info.launcher_id
            }]
        );
        assert_eq!(summary.arbitrage.cats, vec![(asset, price)]);

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
        sim.new_transaction(bundle)?;

        assert!(
            !sim.hinted_coins(taker.puzzle_hash).is_empty(),
            "the offered NFT (and CAT change) should land at the taker"
        );
        assert_eq!(
            balance_at(&sim, cat_puzzle_hash(maker.puzzle_hash, asset)),
            price,
            "maker must receive the requested CAT price"
        );
        Ok(())
    }

    #[test]
    fn requested_nft_assemble_survives_intervening_required_signatures() -> anyhow::Result<()> {
        // Risk-1 guard: a requested-NFT leg's asset info holds an allocator-relative metadata
        // pointer. make_build → required_signatures → make_assemble must all share one ctx and the
        // pointer must survive ctx.take() (which drains only coin spends, not allocator nodes).
        let mut sim = Simulator::new();
        let mut ctx = SpendContext::new();

        let maker = sim.bls(1_000_000);
        let bob = sim.bls(2);
        let requested_nft = mint_nft_for(&mut sim, &mut ctx, &bob, 300)?;

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
            cats: vec![],
            nfts: vec![(
                requested_nft.info.launcher_id,
                crate::NftAssetInfo::new(
                    requested_nft.info.metadata,
                    requested_nft.info.metadata_updater_puzzle_hash,
                    requested_nft.info.royalty_puzzle_hash,
                    requested_nft.info.royalty_basis_points,
                ),
            )],
        };

        // maker_offer runs build → required_signatures (inside sign_for_sim) → assemble in one ctx.
        let offer_str = maker_offer(
            &mut ctx,
            offered,
            requested,
            0,
            std::slice::from_ref(&maker.sk),
        )?;
        assert!(offer_str.starts_with("offer1"));

        let summary = summarize(&offer_str)?;
        assert_eq!(
            summary.requested,
            vec![OfferAsset::Nft {
                launcher_id: requested_nft.info.launcher_id
            }]
        );
        Ok(())
    }

    #[test]
    fn take_build_rejects_non_offer_before_touching_funds() {
        let mut ctx = SpendContext::new();
        let funds = TakerFunds {
            change_puzzle_hash: Bytes32::default(),
            owner_keys: IndexMap::new(),
            xch_coins: vec![],
            cat_coins: vec![],
            nfts: vec![],
            _pd: PhantomData,
        };
        let err = take_build(&mut ctx, "definitely not an offer", funds, 0).unwrap_err();
        assert!(matches!(&err, Error::Decode(m) if m.contains("not a Chia offer")));
    }

    #[test]
    fn take_build_rejects_insufficient_funds() -> anyhow::Result<()> {
        let mut sim = Simulator::new();
        let mut ctx = SpendContext::new();

        let maker = sim.bls(0);
        let taker = sim.bls(0);
        let (maker_cat, asset) = issue_cat_to(&mut sim, &mut ctx, &maker, 80_000)?;

        let offered = OfferedSide {
            change_puzzle_hash: maker.puzzle_hash,
            owner_keys: owner_keys(&maker),
            xch_coins: vec![],
            cat_coins: vec![maker_cat],
            nfts: vec![],
            offer_xch: 0,
            offer_cats: vec![(asset, 80_000)],
            _pd: PhantomData,
        };
        let requested = RequestedSide {
            payee_puzzle_hash: maker.puzzle_hash,
            xch: 50_000,
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

        // The taker brings no XCH — take_build must refuse with a named shortfall.
        let funds = TakerFunds {
            change_puzzle_hash: taker.puzzle_hash,
            owner_keys: owner_keys(&taker),
            xch_coins: vec![],
            cat_coins: vec![],
            nfts: vec![],
            _pd: PhantomData,
        };
        let err = take_build(&mut ctx, &offer_str, funds, 0).unwrap_err();
        assert!(matches!(&err, Error::InvalidInput(m) if m.contains("insufficient XCH")));
        Ok(())
    }
}
