//! The crate error taxonomy.
//!
//! Every fallible operation in dig-offers returns [`Result`], whose error is [`Error`]. The
//! variants separate the failure sources a pure, key-free offer builder can hit: a lower-level
//! driver failure while constructing a spend, a signer failure while computing the required
//! signatures, a malformed `offer1…` string, a combine that merges two incompatible offers, and
//! caller-supplied input that cannot produce a valid offer.

use chia_wallet_sdk::driver::DriverError;
use chia_wallet_sdk::signer::SignerError;

/// The result of a dig-offers operation.
pub type Result<T> = std::result::Result<T, Error>;

/// Everything that can go wrong while building, taking, combining, cancelling, or inspecting a
/// Chia offer.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    /// A failure in the underlying chia-wallet-sdk driver while constructing a spend
    /// (allocation, currying, settlement assembly, coin selection inside the action system).
    #[error("driver error: {0}")]
    Driver(#[from] DriverError),

    /// A failure while computing the BLS signatures a coin spend requires.
    #[error("signer error: {0}")]
    Signer(#[from] SignerError),

    /// A malformed offer string: not a bech32 `offer1…`, or a payload that does not decode to a
    /// valid offer spend bundle. The message states the precise fault.
    #[error("decode error: {0}")]
    Decode(String),

    /// Two offers cannot be combined: they share an offered coin, or their asset metadata
    /// conflicts. The message states the conflict.
    #[error("incompatible offers: {0}")]
    Incompatible(String),

    /// Caller-supplied input that cannot produce a valid offer (an empty side, a zero requested
    /// amount, or funds too small to cover what is offered/taken). The message states the
    /// precise violation, including any shortfall.
    #[error("invalid input: {0}")]
    InvalidInput(String),
}

impl Error {
    /// Construct an [`Error::InvalidInput`] from any displayable message.
    pub(crate) fn invalid(message: impl Into<String>) -> Self {
        Error::InvalidInput(message.into())
    }

    /// Construct an [`Error::Decode`] from any displayable message.
    pub(crate) fn decode(message: impl Into<String>) -> Self {
        Error::Decode(message.into())
    }

    /// Construct an [`Error::Incompatible`] from any displayable message.
    pub(crate) fn incompatible(message: impl Into<String>) -> Self {
        Error::Incompatible(message.into())
    }
}
