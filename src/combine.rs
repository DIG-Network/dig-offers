//! Combine several one-sided offers into a single larger one-sided offer.
//!
//! [`combine`] merges the offered coins and requested payments of every input offer into one
//! offer a single taker settles atomically (all legs or none). It is a pure re-assembly: no new
//! signing, no custody boundary — each input's maker signature is already present and is carried
//! through unchanged.
//!
//! Combining is refused when two inputs would conflict: they share an offered coin (double-spend),
//! or their asset metadata disagrees. Either fault fails the whole combine, so a partial merge is
//! never produced.

use std::collections::HashSet;

use chia_protocol::{Bytes32, SpendBundle};
use chia_wallet_sdk::driver::{encode_offer, SpendContext};

use crate::error::{Error, Result};
use crate::hydrate::{decode, parse};

/// Combine `offers` into one `offer1…` string a single taker can settle atomically.
///
/// Requires at least two offers. Errors ([`Error::Incompatible`]) if any two share an offered
/// coin or carry conflicting asset metadata, and ([`Error::Decode`]) if any member is malformed —
/// the combine is atomic, so one bad member fails the whole operation.
pub fn combine(offers: &[&str]) -> Result<String> {
    if offers.len() < 2 {
        return Err(Error::invalid("combine requires at least two offers"));
    }

    let bundles = offers
        .iter()
        .map(|s| decode(s))
        .collect::<Result<Vec<_>>>()?;
    reject_shared_offered_coins(&bundles)?;

    let mut ctx = SpendContext::new();
    let mut combined = parse(&mut ctx, &bundles[0])?;
    for bundle in &bundles[1..] {
        let next = parse(&mut ctx, bundle)?;
        combined
            .extend(next)
            .map_err(|e| Error::incompatible(format!("offers cannot be merged: {e}")))?;
    }

    let spend_bundle = combined.to_spend_bundle(&mut ctx).map_err(Error::Driver)?;
    encode_offer(&spend_bundle).map_err(|e| Error::decode(format!("could not encode offer: {e}")))
}

/// Fail if any offered coin (a real, non-phantom input coin) appears in more than one offer.
///
/// The phantom carriers that encode requested payments have a zero parent coin id; they are not
/// spent coins, so they are excluded from the conflict set.
fn reject_shared_offered_coins(bundles: &[SpendBundle]) -> Result<()> {
    let mut seen = HashSet::new();
    for bundle in bundles {
        for coin_spend in &bundle.coin_spends {
            if coin_spend.coin.parent_coin_info == Bytes32::default() {
                continue;
            }
            if !seen.insert(coin_spend.coin.coin_id()) {
                return Err(Error::incompatible(
                    "two offers spend the same offered coin (double-spend)",
                ));
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use chia_sdk_test::Simulator;
    use chia_wallet_sdk::driver::SpendContext;

    use crate::test_support::sample_cat_for_xch;
    use crate::{combine, summarize, Error, OfferAsset};

    #[test]
    fn combine_requires_at_least_two_offers() -> anyhow::Result<()> {
        let mut sim = Simulator::new();
        let mut ctx = SpendContext::new();
        let (offer, _m, _a) = sample_cat_for_xch(&mut sim, &mut ctx, 80_000, 50_000)?;
        assert!(matches!(combine(&[&offer]), Err(Error::InvalidInput(_))));
        Ok(())
    }

    #[test]
    fn combine_rejects_shared_offered_coin() -> anyhow::Result<()> {
        let mut sim = Simulator::new();
        let mut ctx = SpendContext::new();
        let (offer, _m, _a) = sample_cat_for_xch(&mut sim, &mut ctx, 80_000, 50_000)?;
        // The same offer twice shares every offered coin — a double-spend.
        let err = combine(&[&offer, &offer]).unwrap_err();
        assert!(matches!(&err, Error::Incompatible(m) if m.contains("same offered coin")));
        Ok(())
    }

    #[test]
    fn combine_rejects_a_malformed_member() -> anyhow::Result<()> {
        let mut sim = Simulator::new();
        let mut ctx = SpendContext::new();
        let (offer, _m, _a) = sample_cat_for_xch(&mut sim, &mut ctx, 80_000, 50_000)?;
        assert!(matches!(
            combine(&[&offer, "not an offer"]),
            Err(Error::Decode(_))
        ));
        Ok(())
    }

    #[test]
    fn combine_merges_two_non_conflicting_offers() -> anyhow::Result<()> {
        let mut sim = Simulator::new();
        let mut ctx = SpendContext::new();
        let (offer_a, _m, asset_a) = sample_cat_for_xch(&mut sim, &mut ctx, 80_000, 50_000)?;
        let (offer_b, _n, asset_b) = sample_cat_for_xch(&mut sim, &mut ctx, 70_000, 40_000)?;

        let combined = combine(&[&offer_a, &offer_b])?;
        assert!(combined.starts_with("offer1"));

        // The combined one-sided offer carries both offered CAT legs and both requested XCH totals.
        let summary = summarize(&combined)?;
        assert!(summary.offered.contains(&OfferAsset::Cat {
            asset_id: asset_a,
            amount: 80_000
        }));
        assert!(summary.offered.contains(&OfferAsset::Cat {
            asset_id: asset_b,
            amount: 70_000
        }));
        assert_eq!(
            summary.requested,
            vec![OfferAsset::Xch(90_000)],
            "requested XCH totals combine"
        );
        Ok(())
    }
}
