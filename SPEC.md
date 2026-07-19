# dig-offers — normative specification (genesis)

> Status: **genesis scaffold (v0.0.0)**. This file states the contract dig-offers v0.1.0 MUST
> satisfy. The v0.1.0 PR (DIG-Network/dig_ecosystem#1226) fills in the byte-level detail as the
> builders land; this scaffold fixes the invariants that MUST NOT change.

## Purpose

`dig-offers` is the canonical Chia **offers** SpendBundle-builder for the DIG ecosystem: make,
take, combine, cancel, and summarize offers over XCH, CAT, and NFT assets, per the Chia
settlement-payment model (CHIP-0023 / CHIP-0024).

## Custody invariants (MUST)

1. **Key-free.** No function accepts, stores, derives, or returns a secret key. Signing inputs
   are public keys only; signatures are produced by the caller.
2. **No signing.** dig-offers NEVER produces a BLS signature. It reports the exact
   `RequiredSignature`s (public key + message) a caller must sign, exactly as `dig-nft` does.
3. **No network.** dig-offers NEVER performs I/O. Coins (with lineage proofs) and chain context
   (the `agg_sig_me` genesis challenge) are supplied by the caller.
4. **No broadcast.** dig-offers NEVER pushes a transaction; it returns coin spends / offer
   strings / spend bundles for the caller to broadcast under its own gate.
5. **Identity-agnostic (#908).** Builders take addresses / puzzle hashes / asset ids only —
   never a DID, never raw key material.

## Correctness invariants (MUST)

- **Nonce agreement.** make and take derive the SAME offer nonce from the SAME offered-coin-id
  set (`nonce = tree_hash(sort_ascending(offered_coin_ids))`), so settlement announcements match.
- **Requested side is asserted, not self-funded.** The requested side of a make is a puzzle
  announcement assertion (+ phantom carrier), NOT a settle action — the maker must not pay both
  sides. (Built on the high-level `chia-wallet-sdk` `Offer`/`RequestedPayments` API, which
  encodes this correctly.)
- **No over/under-payment.** A taken offer settles exactly the requested amounts; both parties'
  assets cross over atomically or the whole transaction reverts.
- **Stable offer id.** The same offer maps to the same id deterministically.
- **Backwards-compatible decode.** The offer decoder accepts every valid current-format
  `offer1…` string.

## Acceptance bar

A two-party simulator test (`chia-sdk-test`): a maker builds+assembles an offer, a distinct
taker decodes+takes it, `Simulator::new_transaction` accepts the combined bundle, and BOTH
sides' balances land — proven in both directions (offer CAT → receive XCH; offer XCH → receive
CAT; offer NFT → receive CAT), plus cancel, combine, and malformed-offer errors. Real broadcast
is never performed in tests.

## Dependency position

dig-offers consumes `dig-cat` and `dig-nft` (both `00-foundation`) and `chia-wallet-sdk`; it sits
strictly ABOVE `00-foundation` in the crate hierarchy and is consumed by higher-level aggregators
(e.g. `dig-wallet-backend`). All dependencies are crates.io releases (no git deps).
