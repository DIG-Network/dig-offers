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

## Custody model

Identity-agnostic (#908): builders take public inputs only. The crate BUILDS spends; the caller
SIGNS. See `SPEC.md` for the normative contract.

## License

Licensed under either of Apache-2.0 or MIT at your option.
