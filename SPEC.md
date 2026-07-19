# dig-offers — normative specification

> Status: **v0.1.0.** This file is the authoritative contract an independent reimplementation of
> dig-offers could be built against. Normative voice: what IS and what an implementation MUST/SHOULD
> do.

## 1. Purpose

`dig-offers` is the canonical Chia **offers** SpendBundle-builder for the DIG ecosystem: make, take,
combine, cancel, and summarize offers over XCH, CAT, and NFT assets, per the Chia settlement-payment
model (CHIP-0023 / CHIP-0024). It is built on the high-level `chia-wallet-sdk` `Offer` /
`RequestedPayments` / action system, which it MUST NOT bypass or re-implement.

## 2. Custody invariants (MUST)

1. **Key-free.** No function accepts, stores, derives, or returns a secret key. Signing inputs are
   public keys only, carried as `owner_keys: IndexMap<Bytes32, PublicKey>` (a coin's p2 puzzle hash
   → its authorizing public key). Signatures are produced by the caller.
2. **No signing.** dig-offers NEVER produces a BLS signature. It reports the exact
   `RequiredSignature`s (public key + message) a caller must sign via
   [`required_signatures`], exactly as `dig-nft` does.
3. **No network.** dig-offers NEVER performs I/O and exposes no `async fn`. Coins (with lineage
   proofs) and chain context (the `agg_sig_me` genesis challenge) are supplied by the caller.
4. **No broadcast.** dig-offers NEVER pushes a transaction; it returns coin spends / offer strings /
   spend bundles for the caller to broadcast under its own gate.
5. **Identity-agnostic (#908).** Builders take addresses / puzzle hashes / asset ids only — never a
   DID, never raw key material.
6. **`#![forbid(unsafe_code)]`.**

### 2.1 Per-function custody boundary

| Function | Output (unsigned artifact) | What re-enters, signed by the caller |
|---|---|---|
| `make_build` | `UnsignedMake.coin_spends` (maker's offered spends) | the signed maker `SpendBundle` → `make_assemble` |
| `make_assemble` | the `offer1…` string (maker half already signed by caller) | — |
| `take_build` | `UnsignedTake.coin_spends` (taker's spends only) | the signed taker `SpendBundle` → `take_combine` |
| `take_combine` | the atomic settlement `SpendBundle` | — |
| `cancel_build` | `UnsignedCancel.coin_spends` (reclaim spends) | signed by the caller, then broadcast |
| `combine`, `summarize`, `offer_id`, `required_signatures` | pure reads / reports; no new signing | — |

## 3. Two-phase state machine + one-context locality (MUST)

Making and taking are two phases with the caller's signing in between; both phases of a flow MUST
execute against the SAME `SpendContext`:

- **make:** `make_build(ctx, …)` → `required_signatures` → caller signs → `make_assemble(ctx, …)`.
- **take:** `take_build(ctx, …)` → `required_signatures` → caller signs → `take_combine(offer, …)`.

A parsed offer and a requested-NFT `AssetInfo` hold allocator-relative pointers (an NFT's metadata is
a `HashedPtr`). `SpendContext::take()` drains only the queued coin spends, not the allocator nodes,
so those pointers survive across the intervening `required_signatures` call. Decoding and building a
take therefore MUST share one context; `take_combine` and `combine` outputs are allocator-free and
MAY be handed out beyond any context.

## 4. Correctness invariants (MUST)

- **Nonce agreement.** The offer nonce is `nonce = tree_hash(sort_ascending(offered_coin_ids))` over
  the FINAL set of offered coins spent into settlement (computed inside `make_build` after coin
  selection). Every requested payment is notarized to this nonce; a taker reads the requested
  payments back out of the decoded offer, so make and take agree by construction.
- **Requested side is asserted, not self-funded.** The requested side of a make is a puzzle-
  announcement assertion (`RequestedPayments::assertions`) plus the phantom carrier emitted by
  `Offer::from_input_spend_bundle` — NEVER an `Action::settle`. A settle action on the requested side
  would make the maker fund both sides (the requested amount would leak as fee at take). `Action::settle`
  appears ONLY when taking. A round-trip MUST leave the maker's offered coin's full change intact.
- **No over/under-payment.** A taken offer settles exactly the requested amounts; both parties'
  assets cross over atomically or the whole transaction reverts.
- **Stable offer id.** See §5.
- **Backwards-compatible decode.** The decoder accepts every valid current-format `offer1…` string.

## 5. Offer id (MUST)

`offer_id(offer_str)` = `SHA-256( decode_offer(offer_str).to_bytes() )` — the SHA-256 of the
**uncompressed** offer spend bundle's canonical Streamable serialization. This equals Chia's
`Offer.name()` / dexie / Sage identifier, so ids are stable and interoperable across the ecosystem
and independent of the bech32 compression. Two encodings of the same offer map to the same id;
distinct offers map to distinct ids.

## 6. Combine semantics (MUST)

`combine(offers)` merges two or more one-sided offers into a single one-sided offer a single taker
settles atomically (all legs or none). It carries each input maker's existing signature through
unchanged (no new signing, no custody boundary). It MUST refuse to combine when:

- two inputs spend the same offered coin (an explicit coin-id conflict pre-check over every real,
  non-phantom input coin) → `Error::Incompatible`;
- two inputs carry conflicting asset metadata (`Offer::extend` returns `Err`) → `Error::Incompatible`;
- any member is malformed → `Error::Decode`.

A combine is atomic: any one faulting member fails the whole operation.

## 7. Settlement conformance (MUST)

- The offered side spends each offered coin into the **settlement puzzle** whose hash is
  `SETTLEMENT_PAYMENT_HASH` (CHIP-0023 / CHIP-0024), sourced from the settlement mod's own
  `mod_hash()`.
- A CAT offered/settled coin uses the settlement puzzle INSIDE the CAT layer
  (`cat_puzzle_hash(tail, SETTLEMENT_PAYMENT_HASH)`); an NFT offered/settled coin is the singleton
  with the settlement inner puzzle. These byte shapes are produced by `chia-wallet-sdk` and MUST NOT
  be hand-rolled; they are the cross-repo contract recorded in the superproject `SYSTEM.md`.

## 8. On-chain memo minimalism (NC-8)

The only memos dig-offers writes on chain are the payee hint on a requested `Payment` and the nonce
carried by a `NotarizedPayment`. No additional payloads are attached.

## 9. Error taxonomy

`Error` (`Result<T> = std::result::Result<T, Error>`):

- `Driver(DriverError)` — a lower-level chia-wallet-sdk failure while constructing a spend.
- `Signer(SignerError)` — a failure computing required signatures.
- `Decode(String)` — a malformed `offer1…` string or an un-serializable bundle.
- `Incompatible(String)` — a combine conflict (shared coin or conflicting asset metadata).
- `InvalidInput(String)` — a caller input that cannot produce a valid offer (an empty side, a zero
  requested amount, funds too small to cover the offered/taken amounts — with the shortfall named).

## 10. Acceptance bar

A two-party simulator test (`chia-sdk-test`): a maker builds+assembles an offer, a distinct taker
decodes+takes it, `Simulator::new_transaction` accepts the combined bundle, and BOTH sides' balances
land — proven in both directions (offer CAT → receive XCH; offer XCH → receive CAT, keeping change;
offer NFT → receive CAT), plus cancel (reclaim + offer-invalidation), combine, and malformed/
insufficient-funds errors. The crate's own tests act as the signing caller; real broadcast is never
performed in tests. Coverage is CI-gated at ≥80% lines.

## 11. Dependency position

dig-offers consumes `dig-cat` and `dig-nft` (both `00-foundation`) and `chia-wallet-sdk`; it sits
strictly ABOVE `00-foundation` in the crate hierarchy and is consumed by higher-level aggregators
(e.g. `dig-wallet-backend`). All dependencies are crates.io releases (no git deps).
