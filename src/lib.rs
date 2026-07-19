//! # dig-offers — the DIG Network canonical Chia offers expert crate
//!
//! `dig-offers` is a **pure, key-free, network-free** SpendBundle-builder for Chia offers
//! (settlement per CHIP-0023/CHIP-0024). It constructs the exact
//! [`CoinSpend`](chia_protocol::CoinSpend)s for every offer operation — make, take, combine,
//! cancel, and summarize/inspect — over any asset (XCH / CAT / NFT), and reports the exact
//! signatures a caller must produce.
//!
//! ## The custody model (HARD invariants)
//!
//! dig-offers **never holds a secret key, never signs, and never touches the network.** Every
//! builder takes only public inputs (puzzle hashes, asset ids, public keys, and coins with their
//! lineage proofs) and appends unsigned coin spends to a caller-owned
//! [`SpendContext`](chia_wallet_sdk::driver::SpendContext). The consumer signs the messages
//! reported by [`required_signatures`], assembles/combines the `SpendBundle`, and broadcasts. This
//! keeps the signing decision — and the secret key — entirely on the caller's side of the identity
//! boundary (#908).
//!
//! ## The make/take two-phase flow
//!
//! Building and assembling are split so the caller signs BETWEEN them, in ONE shared context:
//!
//! - **make:** [`make_build`] → [`required_signatures`] → caller signs → [`make_assemble`].
//! - **take:** [`take_build`] → [`required_signatures`] → caller signs → [`take_combine`].
//!
//! The two phases of each flow MUST share the same [`SpendContext`], because a parsed/requested
//! NFT carries an allocator-relative metadata pointer that only survives in that context.
//!
//! ## The requested-side rule (no self-fund)
//!
//! A make's requested side is an assertion plus a phantom carrier — never a settle action — so the
//! maker never funds both sides of its own offer. Settle actions appear only when taking. See
//! `SPEC.md` for the normative contract.

#![forbid(unsafe_code)]

mod cancel;
mod combine;
mod error;
mod hydrate;
mod make;
mod offer_id;
mod sign;
mod summarize;
mod take;
mod types;

#[cfg(test)]
mod test_support;

pub use cancel::cancel_build;
pub use combine::combine;
pub use error::{Error, Result};
pub use hydrate::{decode, parse};
pub use make::{make_assemble, make_build};
pub use offer_id::offer_id;
pub use sign::required_signatures;
pub use summarize::summarize;
pub use take::{take_build, take_combine};
pub use types::{
    OfferAsset, OfferCost, OfferSummary, OfferedSide, RequestedSide, TakerFunds, UnsignedCancel,
    UnsignedMake, UnsignedTake,
};

// Re-exports so a consumer need not depend on the SDK directly for the common surface.
pub use chia_wallet_sdk::driver::{
    decode_offer, encode_offer, AssetInfo, Cat, CatAssetInfo, Nft, NftAssetInfo, Offer,
    RequestedPayments, SpendContext,
};
pub use chia_wallet_sdk::signer::RequiredSignature;

/// The crate's semantic version, surfaced so a consumer can record which builder version produced
/// a spend.
#[must_use]
pub fn version() -> &'static str {
    env!("CARGO_PKG_VERSION")
}

#[cfg(test)]
mod tests {
    #[test]
    fn version_is_reported() {
        assert!(!super::version().is_empty());
    }
}
