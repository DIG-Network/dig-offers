//! Decode an `offer1…` string, and parse it into a spendable [`Offer`] within a caller-owned
//! [`SpendContext`].
//!
//! These two primitives enforce **one-context locality**: a parsed [`Offer`] holds
//! allocator-relative pointers (an offered NFT's metadata is a `HashedPtr`), so any build that
//! reconstructs those assets — taking, cancelling, combining — MUST parse and build in the SAME
//! context. Callers that need both steps use [`decode`] then [`parse`] against one `ctx`.

use chia_protocol::SpendBundle;
use chia_wallet_sdk::driver::{decode_offer, Offer, SpendContext};

use crate::error::{Error, Result};

/// Decode a bech32 `offer1…` string into the maker's [`SpendBundle`].
///
/// Rejects, with a clear message, anything that is not a valid current-format Chia offer — so a
/// caller surfaces an honest "this isn't an offer" rather than a cryptic failure deep in a build.
/// This is a pure codec step: it allocates nothing in a build context and performs no I/O.
pub fn decode(offer_str: &str) -> Result<SpendBundle> {
    let trimmed = offer_str.trim();
    if !trimmed.starts_with("offer1") {
        return Err(Error::decode(
            "not a Chia offer: expected a bech32 string starting with 'offer1'",
        ));
    }
    decode_offer(trimmed).map_err(|e| Error::decode(format!("invalid offer: {e}")))
}

/// Parse a decoded [`SpendBundle`] into a spendable [`Offer`] within `ctx`.
///
/// The offer's parsed pointers (offered NFT metadata) are valid only for `ctx`'s allocator, so a
/// build that reconstructs offered assets must use this SAME `ctx`.
pub fn parse(ctx: &mut SpendContext, spend_bundle: &SpendBundle) -> Result<Offer> {
    Offer::from_spend_bundle(ctx, spend_bundle)
        .map_err(|e| Error::decode(format!("could not parse offer: {e}")))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decode_rejects_non_offer_prefix() {
        let err = decode("hello world").unwrap_err();
        assert!(matches!(&err, Error::Decode(m) if m.contains("not a Chia offer")));
    }

    #[test]
    fn decode_rejects_blank_string() {
        assert!(matches!(decode("   "), Err(Error::Decode(_))));
    }

    #[test]
    fn decode_rejects_malformed_payload() {
        // Correct prefix but a bad bech32 payload fails at decode, not with a panic.
        assert!(matches!(decode("offer1qqzh3w"), Err(Error::Decode(_))));
    }
}
