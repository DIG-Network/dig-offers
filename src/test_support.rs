//! Shared test helpers: a TEST-ONLY signing bridge and simulator fixtures.
//!
//! dig-offers never signs in production. These helpers act as the *caller* that signs — the role
//! the crate's custody boundary hands to the consumer — so the built spends can be driven onto the
//! `chia-sdk-test` simulator and settled end-to-end. Nothing here is compiled into the crate's
//! non-test surface.

use chia_protocol::{Bytes32, SpendBundle};
use chia_sdk_test::{sign_transaction, BlsPairWithCoin, Simulator};
use chia_wallet_sdk::driver::{
    Action, Cat, Id, Nft, Relation, SpendContext, Spends, StandardLayer,
};
use chia_wallet_sdk::prelude::{SecretKey, Signature};
use chia_wallet_sdk::types::{Conditions, TESTNET11_CONSTANTS};
use indexmap::{indexmap, IndexMap};

use crate::sign::required_signatures;

/// Sign `coin_spends` for the TESTNET11 simulator, first asserting the crate's own
/// [`required_signatures`] report is non-empty and consistent with what the signer will sign.
///
/// This is the TEST-ONLY realization of the custody boundary: dig-offers reports the required
/// signatures, and this stand-in caller produces them.
pub(crate) fn sign_for_sim(
    coin_spends: &[chia_protocol::CoinSpend],
    secret_keys: &[SecretKey],
) -> anyhow::Result<Signature> {
    let reported =
        required_signatures(coin_spends, TESTNET11_CONSTANTS.agg_sig_me_additional_data)?;
    assert!(
        !reported.is_empty(),
        "a spend must report required signatures"
    );
    Ok(sign_transaction(coin_spends, secret_keys)?)
}

/// Issue `amount` of a fresh CAT to `owner`'s puzzle hash in the simulator, returning the
/// spendable [`Cat`] (with lineage proof) and its asset id.
pub(crate) fn issue_cat_to(
    sim: &mut Simulator,
    ctx: &mut SpendContext,
    owner: &BlsPairWithCoin,
    amount: u64,
) -> anyhow::Result<(Cat, Bytes32)> {
    let funding = sim.new_coin(owner.puzzle_hash, amount);
    let p2 = StandardLayer::new(owner.pk);
    let hint = ctx.hint(owner.puzzle_hash)?;
    let (issue, cats) = Cat::issue_with_coin(
        ctx,
        funding.coin_id(),
        amount,
        Conditions::new().create_coin(owner.puzzle_hash, amount, hint),
    )?;
    p2.spend(ctx, funding, issue)?;
    let asset_id = cats[0].info.asset_id;
    sim.spend_coins(ctx.take(), std::slice::from_ref(&owner.sk))?;
    Ok((cats[0], asset_id))
}

/// Mint a royalty NFT owned by `owner` (spending `owner.coin`) and settle it on chain, returning
/// the spendable [`Nft`] (valid in `ctx`'s allocator for a later build).
pub(crate) fn mint_nft_for(
    sim: &mut Simulator,
    ctx: &mut SpendContext,
    owner: &BlsPairWithCoin,
    royalty_basis_points: u16,
) -> anyhow::Result<Nft> {
    let mut spends = Spends::new(owner.puzzle_hash);
    spends.add(owner.coin);
    let deltas = spends.apply(
        ctx,
        &[Action::mint_empty_royalty_nft(
            owner.puzzle_hash,
            royalty_basis_points,
        )],
    )?;
    let outputs = spends.finish_with_keys(
        ctx,
        &deltas,
        Relation::AssertConcurrent,
        &indexmap! { owner.puzzle_hash => owner.pk },
    )?;
    let nft = outputs.nfts[&Id::New(0)];
    sim.spend_coins(ctx.take(), std::slice::from_ref(&owner.sk))?;
    Ok(nft)
}

/// A single-address `owner_keys` map: the standard (p2) puzzle hash → its public key.
pub(crate) fn owner_keys(
    owner: &BlsPairWithCoin,
) -> IndexMap<Bytes32, chia_wallet_sdk::prelude::PublicKey> {
    indexmap! { owner.puzzle_hash => owner.pk }
}

/// Combine a signed maker/taker `SpendBundle`'s pieces for readability in tests.
pub(crate) fn signed_bundle(
    coin_spends: Vec<chia_protocol::CoinSpend>,
    signature: Signature,
) -> SpendBundle {
    SpendBundle::new(coin_spends, signature)
}

/// Drive the full maker flow through the PUBLIC API: build → sign (test caller) → assemble,
/// returning the `offer1…` string. `secret_keys` stands in for the caller that signs.
pub(crate) fn maker_offer(
    ctx: &mut SpendContext,
    offered: crate::OfferedSide<'_>,
    requested: crate::RequestedSide,
    fee: u64,
    secret_keys: &[SecretKey],
) -> anyhow::Result<String> {
    let unsigned = crate::make_build(ctx, offered, requested, fee)?;
    assert!(
        unsigned
            .coin_spends
            .iter()
            .all(|cs| cs.coin.parent_coin_info != Bytes32::default()),
        "make_build must produce only real (non-phantom) maker spends"
    );
    let signature = sign_for_sim(&unsigned.coin_spends, secret_keys)?;
    Ok(crate::make_assemble(
        ctx,
        signed_bundle(unsigned.coin_spends, signature),
        unsigned.requested_payments,
        unsigned.requested_asset_info,
    )?)
}

/// Build a standard one-sided offer that offers `offered_cat` of a fresh CAT and requests
/// `requested_xch` mojos to the maker, returning the `offer1…` string, the maker, and the asset id.
pub(crate) fn sample_cat_for_xch(
    sim: &mut Simulator,
    ctx: &mut SpendContext,
    offered_cat: u64,
    requested_xch: u64,
) -> anyhow::Result<(String, BlsPairWithCoin, Bytes32)> {
    let maker = sim.bls(0);
    let (maker_cat, asset) = issue_cat_to(sim, ctx, &maker, offered_cat)?;
    let offered = crate::OfferedSide {
        change_puzzle_hash: maker.puzzle_hash,
        owner_keys: owner_keys(&maker),
        xch_coins: vec![],
        cat_coins: vec![maker_cat],
        nfts: vec![],
        offer_xch: 0,
        offer_cats: vec![(asset, offered_cat)],
        _pd: std::marker::PhantomData,
    };
    let requested = crate::RequestedSide {
        payee_puzzle_hash: maker.puzzle_hash,
        xch: requested_xch,
        cats: vec![],
        nfts: vec![],
    };
    let offer_str = maker_offer(ctx, offered, requested, 0, std::slice::from_ref(&maker.sk))?;
    Ok((offer_str, maker, asset))
}

/// Drive the full taker flow through the PUBLIC API: build → sign (test caller) → combine,
/// returning the atomic settlement `SpendBundle`.
pub(crate) fn taker_settle(
    ctx: &mut SpendContext,
    offer_str: &str,
    funds: crate::TakerFunds<'_>,
    fee: u64,
    secret_keys: &[SecretKey],
) -> anyhow::Result<SpendBundle> {
    let unsigned = crate::take_build(ctx, offer_str, funds, fee)?;
    let signature = sign_for_sim(&unsigned.coin_spends, secret_keys)?;
    Ok(crate::take_combine(
        unsigned.offer,
        signed_bundle(unsigned.coin_spends, signature),
    ))
}
