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
//! builder takes only public inputs (puzzle hashes, asset ids, public keys, and coins with
//! their lineage proofs) and appends unsigned coin spends to a caller-owned
//! [`SpendContext`](chia_wallet_sdk::driver::SpendContext). The consumer signs the messages
//! reported by the crate's `required_signatures`, assembles/combines the `SpendBundle`, and
//! broadcasts. This keeps the signing decision — and the secret key — entirely on the caller's
//! side of the identity boundary (#908).
//!
//! ## Genesis scaffold
//!
//! This is the v0.0.0 genesis scaffold. The v0.1.0 foundation (the type surface, the error
//! taxonomy, offer decode/summarize, and the make/take/combine/cancel builders) lands via the
//! gated PR that bumps this crate to 0.1.0 (DIG-Network/dig_ecosystem#1226). See `SPEC.md` for
//! the normative contract.

#![forbid(unsafe_code)]

/// The crate's semantic version, surfaced so a consumer can record which builder version
/// produced a spend.
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
