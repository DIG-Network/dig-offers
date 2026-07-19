//! Report the signatures a set of coin spends requires — WITHOUT signing.
//!
//! dig-offers never holds a key and never signs. After a builder produces its unsigned
//! `CoinSpend`s, the caller uses [`required_signatures`] to learn exactly which BLS messages it
//! must sign (each carrying the public key, the raw message, and the appended coin/domain data),
//! signs them with its own key material, and aggregates the result into the `SpendBundle`. This
//! keeps the signing decision — and the secret key — entirely on the caller's side of the
//! identity boundary.

use chia_protocol::{Bytes32, CoinSpend};
use chia_wallet_sdk::signer::{AggSigConstants, RequiredSignature};
use clvmr::Allocator;

use crate::error::Result;

/// Compute every signature `coin_spends` requires, given the network's `agg_sig_me` additional
/// data (the genesis challenge — mainnet or testnet).
///
/// Runs each spend's puzzle to collect its `AGG_SIG_*` conditions and resolves them into
/// [`RequiredSignature`]s. The caller signs each returned message with the matching key and
/// aggregates the signatures. This function performs NO signing and touches no secret.
pub fn required_signatures(
    coin_spends: &[CoinSpend],
    agg_sig_me: Bytes32,
) -> Result<Vec<RequiredSignature>> {
    let mut allocator = Allocator::new();
    let constants = AggSigConstants::new(agg_sig_me);
    Ok(RequiredSignature::from_coin_spends(
        &mut allocator,
        coin_spends,
        &constants,
    )?)
}
