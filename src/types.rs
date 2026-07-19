//! The public value types every dig-offers builder speaks in.
//!
//! These types are deliberately **key-free**: every side of an offer carries its participant's
//! *public* keys (an `IndexMap<Bytes32, PublicKey>` from a coin's p2 puzzle hash to the public
//! key that authorizes it), never a secret. A builder consumes these, appends unsigned
//! `CoinSpend`s to a caller-owned [`SpendContext`](chia_wallet_sdk::driver::SpendContext), and
//! returns the unsigned artifact for the caller to sign.

use std::marker::PhantomData;

use chia_protocol::{Bytes32, Coin, CoinSpend};
use chia_wallet_sdk::driver::{AssetInfo, Cat, Nft, NftAssetInfo, Offer, RequestedPayments};
use chia_wallet_sdk::prelude::PublicKey;
use indexmap::IndexMap;

/// A single asset leg, used to describe what an offer offers or requests in a read-only
/// [`OfferSummary`]. Amounts are the asset's base units (mojos for XCH, base units for CATs);
/// an NFT is always quantity one and identified by its launcher id.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OfferAsset {
    /// XCH, amount in mojos.
    Xch(u64),
    /// A CAT identified by its `asset_id` (TAIL hash), amount in base units.
    Cat {
        /// The CAT's asset id (TAIL hash).
        asset_id: Bytes32,
        /// The amount in the CAT's base units.
        amount: u64,
    },
    /// An NFT identified by its launcher id (quantity is always one).
    Nft {
        /// The NFT singleton's launcher id — its stable on-chain identity.
        launcher_id: Bytes32,
    },
}

impl OfferAsset {
    /// The fungible amount this leg carries (zero for an NFT, which is not fungible).
    #[must_use]
    pub fn amount(&self) -> u64 {
        match self {
            OfferAsset::Xch(amount) | OfferAsset::Cat { amount, .. } => *amount,
            OfferAsset::Nft { .. } => 0,
        }
    }
}

/// The maker's side of a make-offer: the coins it spends into the settlement puzzle (its funding
/// XCH, CAT, and NFT coins), the fungible amounts it offers, and where change returns.
///
/// The funding coins fund the offered amounts (plus any network fee); surplus returns to
/// `change_puzzle_hash`. `owner_keys` maps every funding coin's p2 (inner) puzzle hash to the
/// **public** key that authorizes it — the builder never sees a secret. `nfts` are the NFTs to
/// offer (each spent whole into settlement); they must be parsed in the SAME
/// [`SpendContext`](chia_wallet_sdk::driver::SpendContext) passed to `make_build`, since an NFT
/// carries an allocator-relative metadata pointer.
pub struct OfferedSide<'a> {
    /// Where offered-coin change (and fee surplus) returns.
    pub change_puzzle_hash: Bytes32,
    /// Every funding coin's p2 puzzle hash → the public key authorizing it.
    pub owner_keys: IndexMap<Bytes32, PublicKey>,
    /// Spendable XCH coins available to fund the offered XCH and the fee.
    pub xch_coins: Vec<Coin>,
    /// Spendable CAT coins (with lineage proofs) available to fund the offered CAT legs.
    pub cat_coins: Vec<Cat>,
    /// The NFTs to offer, each spent whole into settlement (parsed in the build context).
    pub nfts: Vec<Nft>,
    /// The XCH (mojos) to offer.
    pub offer_xch: u64,
    /// The CATs to offer, as `(asset_id, amount)` in base units.
    pub offer_cats: Vec<(Bytes32, u64)>,
    /// Ties the borrowed NFTs' lifetime to the build context.
    pub _pd: PhantomData<&'a ()>,
}

/// The maker's requested side of a make-offer: what the taker must pay, and where it is paid.
///
/// Every requested payment is notarized to the offer's nonce and asserted (never self-funded).
/// A requested NFT needs its [`NftAssetInfo`] (metadata pointer, royalty) so the settlement
/// puzzle hash can be rebuilt correctly.
pub struct RequestedSide {
    /// The address every requested payment is paid to (the maker's receive address).
    pub payee_puzzle_hash: Bytes32,
    /// The XCH (mojos) requested.
    pub xch: u64,
    /// The CATs requested, as `(asset_id, amount)` in base units.
    pub cats: Vec<(Bytes32, u64)>,
    /// The NFTs requested, as `(launcher_id, asset_info)` — the asset info rebuilds settlement.
    pub nfts: Vec<(Bytes32, NftAssetInfo)>,
}

/// The taker's funding coins for taking an offer: its spendable XCH, CAT, and NFT coins, and
/// where change / received assets return.
///
/// `owner_keys` maps every funding coin's p2 puzzle hash to the **public** key authorizing it;
/// `change_puzzle_hash` receives change, surplus, and the assets the offer delivers to the taker.
/// NFTs the taker gives up (for an NFT-for-NFT take) are supplied in `nfts`, parsed in the SAME
/// context passed to `take_build`.
pub struct TakerFunds<'a> {
    /// Where change, surplus, and received assets return.
    pub change_puzzle_hash: Bytes32,
    /// Every funding coin's p2 puzzle hash → the public key authorizing it.
    pub owner_keys: IndexMap<Bytes32, PublicKey>,
    /// Spendable XCH coins available to fund the requested XCH and the fee.
    pub xch_coins: Vec<Coin>,
    /// Spendable CAT coins (with lineage proofs) available to fund requested CAT payments.
    pub cat_coins: Vec<Cat>,
    /// NFTs the taker gives up to fulfil a requested-NFT leg (parsed in the build context).
    pub nfts: Vec<Nft>,
    /// Ties the borrowed NFTs' lifetime to the build context.
    pub _pd: PhantomData<&'a ()>,
}

/// What a taker must fund to take an offer: the requested-over-offered surplus (the offer's
/// arbitrage). NFTs the taker receives are not a cost; NFTs the taker gives up are expressed by
/// the requested-NFT legs, not here.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct OfferCost {
    /// XCH (mojos) the taker must pay.
    pub xch: u64,
    /// Per-asset-id CAT base units the taker must pay.
    pub cats: Vec<(Bytes32, u64)>,
}

/// A read-only summary of an `offer1…` string: what it offers, what it requests, the taker's
/// arbitrage cost, and any NFT royalties it carries. Produced by
/// [`summarize`](crate::summarize) without committing to the offer.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct OfferSummary {
    /// Assets the offer gives the taker.
    pub offered: Vec<OfferAsset>,
    /// Assets the offer asks the taker to pay.
    pub requested: Vec<OfferAsset>,
    /// The net the taker must fund (the requested-over-offered surplus).
    pub arbitrage: OfferCost,
    /// Royalty legs the offer carries: `(NFT launcher id, royalty basis points)`.
    pub royalties: Vec<(Bytes32, u16)>,
}

/// The unsigned artifact of [`make_build`](crate::make_build): the coin spends the caller must
/// sign, and the requested-payment context to hand back to [`make_assemble`](crate::make_assemble)
/// once signed.
///
/// **The custody boundary.** `coin_spends` are unsigned; the caller computes their required
/// signatures with [`required_signatures`](crate::required_signatures), signs, aggregates into a
/// `SpendBundle`, and passes that plus `requested_payments` + `requested_asset_info` back to
/// `make_assemble`. dig-offers never produces the signature.
#[derive(Debug)]
pub struct UnsignedMake {
    /// The unsigned coin spends the caller must sign.
    pub coin_spends: Vec<CoinSpend>,
    /// The requested payments (what the taker will pay), carried to `make_assemble`.
    pub requested_payments: RequestedPayments,
    /// The requested asset metadata (rebuilds settlement for requested CAT/NFT legs).
    pub requested_asset_info: AssetInfo,
    /// The offer nonce derived from the offered coins — reported for audit; already baked into
    /// the requested payments.
    pub nonce: Bytes32,
}

/// The unsigned artifact of [`take_build`](crate::take_build): the taker's own coin spends to
/// sign, the maker's already-signed offer, and the cost the take funds.
///
/// **The custody boundary.** `coin_spends` are the TAKER's unsigned spends only; the maker's half
/// is already signed inside `offer`. The caller signs `coin_spends`, wraps them in a `SpendBundle`,
/// and calls [`take_combine`](crate::take_combine) with `offer` to produce the atomic settlement.
#[derive(Debug)]
pub struct UnsignedTake {
    /// The taker's unsigned coin spends (funding + received-asset routing).
    pub coin_spends: Vec<CoinSpend>,
    /// The maker's already-signed offer, carried to `take_combine`.
    pub offer: Offer,
    /// What the taker funds to take the offer (the arbitrage).
    pub cost: OfferCost,
}

/// The unsigned artifact of [`cancel_build`](crate::cancel_build): the reclaim coin spends the
/// maker must sign to invalidate an outstanding offer.
#[derive(Debug)]
pub struct UnsignedCancel {
    /// The unsigned coin spends that reclaim the offered coins to the maker.
    pub coin_spends: Vec<CoinSpend>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn offer_asset_amount_reports_fungible_value() {
        assert_eq!(OfferAsset::Xch(500).amount(), 500);
        assert_eq!(
            OfferAsset::Cat {
                asset_id: Bytes32::default(),
                amount: 42
            }
            .amount(),
            42
        );
        // An NFT is non-fungible — it carries no fungible amount.
        assert_eq!(
            OfferAsset::Nft {
                launcher_id: Bytes32::default()
            }
            .amount(),
            0
        );
    }
}
