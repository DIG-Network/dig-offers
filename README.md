# dig-offers

The DIG Network canonical **Chia offers** expert crate: a pure, key-free, network-free
`SpendBundle`-builder for Chia offers (settlement per CHIP-0023/CHIP-0024) over any asset
(XCH / CAT / NFT).

`dig-offers` constructs the exact `CoinSpend`s for every offer operation and reports the exact
signatures a caller must produce. It **never holds a secret key, never signs, and never touches
the network.** The consumer signs the reported messages, assembles/combines the `SpendBundle`,
and broadcasts.

## Scope

- **make** — build a one-sided offer: OFFER assets (spent into the settlement puzzle) and
  REQUEST assets (asserted, paid to the maker) — returns the unsigned offered coin spends +
  the requested-payment metadata; the caller signs, then `dig-offers` assembles the `offer1…`
  string.
- **take** — accept an offer: build the taker's counter-spends (fund the requested side, claim
  the offered coins), report required signatures, then combine with the maker's signed half.
- **combine** — merge multiple compatible offers into a single settlement.
- **cancel** — reclaim the offered coins of an outstanding offer back to the maker.
- **summarize / inspect** — decode an `offer1…` string into a two-sided summary (offered vs
  requested, arbitrage, royalties) without committing.
- **offer id** — stable, deterministic identifier for persistence + dedup.

## The two-phase flow (make / take)

Building and assembling are split so the caller signs BETWEEN them, in ONE shared
`SpendContext`:

```text
make:  make_build(ctx, offered, requested, fee) -> UnsignedMake
       required_signatures(&unsigned.coin_spends, agg_sig_me)  // caller signs
       make_assemble(ctx, signed_bundle, requested_payments, requested_asset_info) -> "offer1…"

take:  take_build(ctx, offer1_str, funds, fee) -> UnsignedTake
       required_signatures(&unsigned.coin_spends, agg_sig_me)  // caller signs (taker half only)
       take_combine(unsigned.offer, signed_taker_bundle) -> SpendBundle  // atomic settlement
```

The two phases of each flow MUST share the same `SpendContext`: a parsed/requested NFT carries an
allocator-relative metadata pointer that only survives in that context. `take_combine` and
`combine` outputs are allocator-free and safe to hand out.

## Custody model

Identity-agnostic (#908): builders take public inputs only — puzzle hashes, asset ids, coins with
lineage proofs, and `PublicKey`s (`owner_keys: IndexMap<Bytes32, PublicKey>`). The crate BUILDS
unsigned spends and REPORTS the required signatures; the caller SIGNS. No function accepts, stores,
derives, or returns a secret key, and the crate never produces a BLS signature or performs I/O. See
`SPEC.md` for the normative contract.

## Dependency position

`dig-offers` sits above the `00-foundation` layer: it consumes `dig-cat` and `dig-nft` (both
`00-foundation`) and `chia-wallet-sdk`, and is consumed by higher-level aggregators (e.g.
`dig-wallet-backend`). All dependencies are crates.io releases (no git deps).

## License

Licensed under either of Apache-2.0 or MIT at your option.
