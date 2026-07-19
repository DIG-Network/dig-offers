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
