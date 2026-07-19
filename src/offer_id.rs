//! A deterministic, ecosystem-compatible id for an offer.
//!
//! [`offer_id`] is the SHA-256 of the *uncompressed* offer spend bundle's canonical Streamable
//! bytes — the same value Chia's `Offer.name()`, dexie, and Sage use to identify an offer. Two
//! encodings of the same offer therefore map to the same id, and the id is independent of the
//! bech32 compression, so it is stable across the ecosystem.

use chia_protocol::Bytes32;
use chia_sha2::Sha256;
use chia_traits::Streamable;

use crate::error::{Error, Result};
use crate::hydrate::decode;

/// The canonical id of the offer encoded by `offer_str`.
///
/// Computed as `sha256(spend_bundle.to_bytes())` over the decoded (uncompressed) offer bundle's
/// Streamable serialization. Errors if `offer_str` is not a valid offer, or if the bundle cannot
/// be serialized.
pub fn offer_id(offer_str: &str) -> Result<Bytes32> {
    let spend_bundle = decode(offer_str)?;
    let bytes = spend_bundle
        .to_bytes()
        .map_err(|e| Error::decode(format!("could not serialize offer bundle: {e}")))?;
    let mut hasher = Sha256::new();
    hasher.update(&bytes);
    Ok(Bytes32::from(hasher.finalize()))
}

#[cfg(test)]
mod tests {
    use chia_sdk_test::Simulator;
    use chia_wallet_sdk::driver::{decode_offer, encode_offer, SpendContext};

    use crate::offer_id;
    use crate::test_support::sample_cat_for_xch;

    #[test]
    fn offer_id_is_stable_across_re_encoding() -> anyhow::Result<()> {
        let mut sim = Simulator::new();
        let mut ctx = SpendContext::new();
        let (offer_str, _maker, _asset) = sample_cat_for_xch(&mut sim, &mut ctx, 80_000, 50_000)?;

        // Re-encoding the decoded bundle yields another valid encoding of the SAME offer; its id
        // must match — the id is over the uncompressed bundle, independent of the encoding.
        let re_encoded = encode_offer(&decode_offer(&offer_str)?)?;
        assert_eq!(offer_id(&offer_str)?, offer_id(&re_encoded)?);
        Ok(())
    }

    #[test]
    fn distinct_offers_have_distinct_ids() -> anyhow::Result<()> {
        let mut sim = Simulator::new();
        let mut ctx = SpendContext::new();
        let (offer_a, _m, _a) = sample_cat_for_xch(&mut sim, &mut ctx, 80_000, 50_000)?;
        let (offer_b, _n, _b) = sample_cat_for_xch(&mut sim, &mut ctx, 70_000, 40_000)?;
        assert_ne!(offer_id(&offer_a)?, offer_id(&offer_b)?);
        Ok(())
    }

    #[test]
    fn offer_id_rejects_non_offer() {
        assert!(offer_id("not an offer").is_err());
    }
}
