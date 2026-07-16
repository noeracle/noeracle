# Noeracle Architecture

> Status: draft. Last revised: 2026-05-19. Owner: Yahya.
>
> Audience: contributors and auditors. Assumes familiarity with Soroban contract
> development and Stellar transaction semantics.
> Prerequisites: skim [DOCUMENTATION_PLAN.md](DOCUMENTATION_PLAN.md) for project
> phasing; skim [oracle_v0/src/lib.rs](oracle_v0/src/lib.rs) for the contract surface.

---

## 1. What Noeracle is

Noeracle is the **first pull-based price oracle on Stellar**, designed as the
explicit complement to Reflector's push-based oracle infrastructure. The two
projects target different categories:

- **Reflector** — the incumbent Stellar oracle, push-based. `ReflectorPulse`
  publishes price feeds on a uniform ~5-minute cadence with free reads;
  `ReflectorBeam` offers faster updates in return for an XRF invocation fee;
  `Reflector Subscriptions` push threshold-triggered notifications. Reflector
  V3 was independently audited (Code4rena, Oct 2025). Every variant writes to
  on-chain state on a cadence, so the freshness a consumer can act on is
  bounded by Reflector's publish interval and by ledger close time. Optimal
  for displays, anchor UIs, stablecoin mint/redeem flows, slow rebalancing —
  anywhere read freshness up to a few seconds is acceptable.
- **Noeracle** — pull-based oracle. Consumers fetch a freshly signed price
  attestation from Noeracle's off-chain service and bundle it into their own
  Stellar transaction. The on-chain Soroban contract verifies the publisher
  signature, stores the price, and the consumer's application logic executes
  against the just-verified value in the same transaction. The price executed
  against is sub-second-fresh — bounded by signer cadence, not by ledger close
  time. Optimal for perp DEX execution, lending liquidations, oracle-priced AMM
  swaps, options pricing — anywhere sub-second freshness is required.

This separation is by design and is coordinated with the Reflector team.
Some protocols will use both: Reflector for display and informational reads,
Noeracle for execution paths that require fresh-on-tx prices.

A terminology note: "pull" here follows Pyth's usage — the consumer *pulls* a
freshly signed update from an off-chain service and submits it itself. The
Stellar Docs oracle-providers page uses "pull" differently, to mean a consumer
*querying* on-chain state. Developer-facing Noeracle copy should prefer
"on-demand" or "fetch-and-verify" to avoid the collision.

The same Soroban verification contract and the same off-chain signing
infrastructure also serve a second product surface for tokenized real-world
assets (the v2 roadmap): regulated FX feeds with source
attribution, proof-of-reserves attestations for anchored / tokenized issuers,
and NAV publication for tokenized funds. These are pull-shaped naturally
(issuer signs, consumer verifies) and share Noeracle's verification machinery.

---

## 2. Design goals (and non-goals)

**Goals.**

- *Pull-only, by design.* One read mode, one integration pattern, one story.
  The contract, SDK, attestation service, and documentation all converge on
  a single "bring your fresh signed price into your tx" pattern. Push state is
  Reflector's domain.
- *Soroban-native verification.* Every Noeracle claim is verifiable directly
  on Stellar. No Wormhole, no bridge, no sidechain. The trust assumption is
  exactly the publisher set plus Stellar's own consensus.
- *Cost-honesty.* Every supported signature scheme has been measured in host
  isolation (`bench/`) and end-to-end on real testnet/mainnet
  (`scripts/run_oracle_bench.mjs`). Architecture choices are grounded in
  measured CPU + XLM cost.
- *Trivially easy integration.* A Soroban consumer contract should integrate
  Noeracle in under 10 lines of TypeScript SDK code. The SDK is the most
  important deliverable; the contract is supporting infrastructure.
- *Complementary, not competitive.* Noeracle's positioning explicitly cedes
  the push-state read category to Reflector. No friction with the incumbent;
  more total oracle coverage for Stellar developers.

**Non-goals.**

- *Push state operation.* Noeracle does not run a keeper that updates on-chain
  state on a fixed cadence. Consumers who need free reads with bounded
  staleness use Reflector.
- *Cross-chain oracle delivery.* Noeracle does not bridge prices to or from
  other chains.
- *General-purpose programmable oracle.* Noeracle signs and serves data of
  agreed schemas; it is not a Chainlink-functions-style compute layer.
- *Self-custodial consumer key management.* Consumers integrate with their own
  existing wallet stack.

---

## 3. System overview

```
                +-----------------------------------------+
                |    Data sources (off-chain, external)   |
                |  Coinbase / Binance / Kraken / OKX /    |
                |  Bybit (weighted avg, 4-5 sources)      |
                +-------------------+---------------------+
                                    |
                                    v
                +-------------------+---------------------+
                |    Attestation service (off-chain)      |
                |  - polls 5 exchanges every 500 ms       |
                |  - computes per-asset weighted average  |
                |  - signs (asset || price || ts || round)|
                |  - serves /v1/latest/{asset} via HTTPS  |
                |  - exposes SSE stream for subscribers   |
                +-------------------+---------------------+
                                    |
                                    | HTTP GET / SSE
                                    v
                +-------------------+---------------------+
                |        Noeracle TypeScript SDK          |
                |  noeracle.fetchLatest([assets])         |
                |  .toUpdateOp(contractAddress)           |
                +-------------------+---------------------+
                                    |
                                    | prepended op
                                    v
        +---------------------------+-----------------------------+
        |              Consumer's Stellar transaction             |
        |                                                         |
        |  op_1: oracle.update_batch_ed25519_args(signed_prices)  |
        |  op_2: <consumer's own application logic>               |
        +---------------------------+-----------------------------+
                                    |
                                    v
                +-------------------+---------------------+
                |    Soroban contract (on-chain)          |
                |  - verify publisher signatures          |
                |  - check staleness + monotonic round_id |
                |  - write PriceEntry to temp storage     |
                |  - return to op_2 (same tx) for use     |
                +-----------------------------------------+
```

Three logical components: data sources, the off-chain attestation service, and
the on-chain contract. The TypeScript SDK is the seam between off-chain and
on-chain — it's how Soroban application developers actually consume the oracle.

---

## 4. Components

### 4.1 TypeScript SDK (primary integration surface)

The SDK is **the central deliverable** of v0. If integration takes more
than ten lines of TypeScript, pull-only fails. Target shape:

```typescript
import { Noeracle } from "@noeracle/sdk";

const oracle = new Noeracle({ network: "testnet" });

// Fetch fresh signed prices for the assets your tx needs:
const fresh = await oracle.fetchLatest(["BTC/USD", "ETH/USD"]);

// Prepend the verification op to your existing Stellar transaction:
tx.addOperation(fresh.toUpdateOp(oracleContractId));

// Then your normal application op(s):
tx.addOperation(myDexContract.openPosition(...));

await server.sendTransaction(tx);
```

Responsibilities:

- Fetch the latest signed message per asset from the attestation service.
- Subscribe to the SSE stream for low-latency refresh in long-running clients.
- Build the `update_batch_ed25519_args` operation correctly given the latest
  signed prices for one or more assets.
- Expose typed `PriceEntry` and verification error types for consumer code.
- Provide a Rust client helper module (separate package) for Soroban consumer
  contracts that need to call `get_price` for the best-effort cache.

### 4.2 Off-chain attestation service

Implemented under `scripts/keeper/` (v0, shipped 2026-05; batched
per-venue polling since 2026-07). Stateless, recoverable, single-process;
replicated across three Fly.io regions (fra / ams / cdg) since 2026-06 for
egress-IP redundancy.

Responsibilities:

1. **Poll** the 5 exchange APIs (Coinbase Advanced, Binance, Kraken, OKX,
   Bybit) every 500 ms — one batched spot-ticker request per venue covering
   all supported assets, so request rate stays flat as assets grow.
2. **Filter** out sources that haven't refreshed within freshness budget and
   samples beyond N standard deviations.
3. **Compute** the per-asset exchange-weighted average across remaining
   sources.
4. **Sign** the message `asset || price (i128 BE) || timestamp (u64 BE) ||
   round_id (u64 BE)` with the publisher Ed25519 key.
5. **Serve** the latest signed message per asset via `GET /v1/latest/{asset}`
   and an SSE stream at `/v1/stream`.

**No on-chain submission loop.** The attestation service does not push to the
contract on a schedule. The contract state is warmed only as a side effect of
consumer pull-mode usage. This is the central architectural simplification of
pull-only versus the previous push-and-pull design.

### 4.3 On-chain contract

Implemented in [`oracle_v0/src/lib.rs`](oracle_v0/src/lib.rs). Soroban contract,
`crate-type = ["cdylib"]`, built against `wasm32v1-none`.

Production entrypoints:

| Entrypoint                       | Purpose                                                |
|----------------------------------|--------------------------------------------------------|
| `update_batch_ed25519_args`      | Pull verification path — called inline in consumer tx  |
| `get_price`                      | Best-effort cache read (see §7)                        |
| `init` / `set_publishers`        | Admin: publisher pubkey registration                    |

Auxiliary entrypoints in v0 (`update_ed25519_args`, `update_ed25519_stored`,
`update_ed25519_persistent`, `update_bls_agg`, `update_secp256k1`,
`update_via_auth`) exist to ground architectural decisions in measured cost.
They are kept in v0 for benchmarking and reproducibility but will not carry
over to v1.

---

## 5. Data flow: pull verification in a consumer transaction

```
Attestation service                                          Consumer (off-chain)
-------------------                                          --------------------
poll exchanges (every 500 ms)
sign per-asset message                <--------------------- SDK: oracle.fetchLatest(["BTC/USD"])
                                      ---------------------> returns signed message + metadata

                                                              SDK: fresh.toUpdateOp(oracleContractId)
                                                              builds InvokeContract operation

                                                              consumer: tx.addOperation(updateOp)
                                                                        tx.addOperation(appOp)
                                                                        server.sendTransaction(tx)

Soroban network
---------------
ledger close (~5 s)
  |
  v
verify Ed25519 sig(s) against configured publisher set
check staleness window: env.ledger().timestamp() - timestamp < 60 s
check monotonic round_id: new > stored
write PriceEntry to temp storage
extend TTL
execute consumer's appOp using just-written price
```

**Freshness guarantee.** The price that the consumer's tx executes against was
signed at most ~500 ms before the consumer called `fetchLatest`. Stellar's
ledger close time bounds *when* the tx executes, not *what price* it executes
against — the price is whatever the publisher committed to at signing time.

**Replay protections.** Two checks inside the contract entrypoint prevent
abuse on the pull path:

- *Staleness window.* Reject if `env.ledger().timestamp() - timestamp > 60 s`.
  Prevents replay of stale signed prices.
- *Monotonic `round_id`.* Reject if incoming `round_id` ≤ currently-stored
  `round_id` for the asset. Prevents an attacker from overwriting a fresher
  state with an older signed message.

---

## 6. Cryptography

### 6.1 Signature scheme: Ed25519

Ed25519 was selected as the production signature scheme based on measured
Soroban host cost. The bench suite (`bench/src/test.rs`) measures all
production-relevant schemes in isolation with the host budget reset between
calls:

| Scheme               | Decision                                                |
|----------------------|--------------------------------------------------------|
| Ed25519              | **Selected.** Cheapest on Soroban, smallest sigs.       |
| secp256k1 (recover)  | Rejected. ~5.5× more CPU than Ed25519.                  |
| secp256r1 (verify)   | Rejected. ~7× more CPU than Ed25519.                    |
| BLS12-381 aggregate  | Kept as future option for high-publisher-count sets.    |

Measurements reproducible via `cargo test -p noeracle_bench -- --nocapture`
and grounded in real testnet+mainnet fees via `scripts/run_oracle_bench.mjs`.

### 6.2 Signed message format

Used identically in three places (the contract's `build_msg`, the bench tests,
and the JS attestation service / SDK):

```
asset(8 bytes) || price(i128 BE, 16 bytes) || timestamp(u64 BE, 8 bytes) || round_id(u64 BE, 8 bytes)
= 40 bytes total
```

This format is load-bearing: the three implementations must stay in sync or
signatures stop verifying.

### 6.3 BLS domain-separation tags

For the future BLS-aggregate path (v1+ as publisher count grows):

```
G1: BLS_SIG_BLS12381G1_XMD:SHA-256_SSWU_RO_NUL_
G2: BLS_SIG_BLS12381G2_XMD:SHA-256_SSWU_RO_NUL_
```

These are the RFC9380 ciphersuite identifiers Soroban's `hash_to_g1` /
`hash_to_g2` produce against. The JS side mirrors these constants.

---

## 7. Storage model

In pull-only operation, contract storage has a different role than in a
push-based oracle. The state is **not pre-warmed by a keeper**. It is written
only as a side effect of consumer pull-mode transactions: when consumer A pulls
a fresh price and submits a verifying tx, the verified price is stored, and a
subsequent free `get_price` read by consumer B will return that price until
either it expires from TTL or is overwritten by another consumer's pull.

This produces a deliberate two-tier read surface:

- **`update_batch_ed25519_args(...)` inline in your own tx** — the only
  freshness-*guaranteed* path. Price executes against what was signed within
  the last second. Consumer pays verification cost. Use for execution paths.
- **`get_price(asset)` standalone read** — best-effort cache, free. Returns
  whatever the last pull-mode tx wrote, or `None` if TTL has expired. Freshness
  is opportunistic (high-traffic assets stay warm; low-traffic assets go
  cold). Use for non-execution reads where you'd otherwise use Reflector but
  prefer a Noeracle-side cache.

Storage variants:

| Key                    | Storage    | Purpose                                          |
|------------------------|-----------|--------------------------------------------------|
| `PriceTemp(asset)`     | Temporary | Production cache, written by pull-mode txs       |
| `PricePers(asset)`     | Persistent| Reserved for RWA records (NAV history etc.)     |
| `Publishers`           | Instance  | Configured publisher pubkey set                 |

TTL constants:

| Constant            | Value     | Approximate duration |
|---------------------|-----------|----------------------|
| `TEMP_THRESHOLD`    | 360       | ~30 min              |
| `TEMP_EXTEND`       | 720       | ~1 hr                |
| `PERS_THRESHOLD`    | 60,480    | ~3.5 days            |
| `PERS_EXTEND`       | 120,960   | ~7 days              |

Temp TTL is set well above network minimums so that low-traffic assets retain
a stored value across modest gaps between consumer pulls.

---

## 8. Trust model

### 8.1 v0 (hackathon)

- **One self-operated publisher.** Single Ed25519 key. Compromise = full
  oracle compromise. Mitigated by the staleness window and the prototype-only
  scope (not for production capital).
- **Single admin key.** Owns `init` and `set_publishers`. No timelock.

### 8.2 v1 (mainnet)

- **3-of-5 threshold publisher set.** Configured at contract level. Compromise
  of ≤2 publishers is non-fatal.
- **Multi-publisher composition.** Target mix: 2 self-operated (geographic
  redundancy), 1 market-maker, 1 exchange data desk, 1 Stellar ecosystem
  infrastructure team. Concrete partners named publicly once
  conversations are confirmed.
- **Admin multi-sig.** Upgrade and parameter changes gated by a 3-of-5
  multi-sig distinct from the publisher set.
- **Independent audit** completed and findings remediated.

### 8.3 v2 (decentralized)

- **Stake-backed publishers.** Each publisher locks XLM as economic security.
  Slashing for misreporting and downtime exceeding documented thresholds.
- **Governance handoff.** Multi-sig comprises team members plus ecosystem
  representatives (SDF, publisher rep, consumer-protocol rep).
- **Sustainability.** Per-tx pull fees and RWA subscriptions cover operating
  cost.

---

## 9. Failure modes

| Failure                                                | Consequence                                       | Detection / mitigation                                                                              |
|--------------------------------------------------------|---------------------------------------------------|------------------------------------------------------------------------------------------------------|
| Attestation service down                               | `fetchLatest` fails; pull-mode txs blocked       | Geo-replicated across 3 Fly regions (done); SDK retries with backoff; consumers fall back to Reflector cache |
| Single publisher key compromise (v0)                   | Full oracle compromise                            | v0 is prototype only; production v1 ships with 3-of-5 threshold                                       |
| ≤2 publisher compromise (v1)                           | No protocol impact (threshold not met)            | Detected via signing-rate dashboard; rotate compromised keys via governance                          |
| Exchange data source returns bad data                  | Aggregate includes outlier                         | Service rejects samples beyond N stddev and below freshness threshold                                |
| Stellar network congestion                             | Consumer's tx may not land within freshness window | Consumer retries; SDK auto-fetches fresher attestation on retry                                       |
| Replay of old signed message                           | Stale price written to state                       | Contract enforces 60 s staleness window and monotonic `round_id`                                      |
| Soroban host upgrade breaks signature primitive        | All pull-mode txs fail                             | Snapshot tests (`bench/test_snapshots/`) catch host-cost regressions in CI                           |
| `get_price` returns stale value                        | Consumer using cache reads outdated data           | Documented: `get_price` is best-effort; freshness-sensitive reads must use pull                       |

---

## 10. Cost model

**Pull verification cost** — paid by the consumer, on their own transaction:

- Adds one `update_batch_ed25519_args` operation to the consumer's tx.
- Cost = N × Ed25519 verify CPU + ~96 bytes per signature in tx size.
- For 3-of-5 threshold and 1–4 assets per tx, the overhead is a small fraction
  of the consumer's existing application op cost. Measured numbers published
  in `bench/RESULTS.md`.

**Attestation service cost** — paid by Noeracle:

- Off-chain compute + bandwidth only. No on-chain submission loop and no
  on-chain XLM burn. Funded by the team through v1; from per-tx pull
  fees + RWA subscriptions in v2+.

**Free reads** — `get_price` standalone:

- Zero marginal cost to the consumer.
- Zero marginal cost to Noeracle (state is written as a side effect of paid
  pull-mode txs).

The cost surface scales with freshness demand: consumers who need sub-second
prices pay for them; consumers who tolerate best-effort cache reads pay
nothing.

---

## 11. Versioning and evolution

- **v0** (current): single publisher, prototype, testnet only. Contracts in
  `oracle_v0/`.
- **v1**: multi-publisher threshold, audited, mainnet. Will live
  in `oracle_v1/` to keep v0 untouched as a historical artifact and to allow
  the bench crate to continue measuring against the v0 surface.
- **v2**: stake + slashing extensions. May reuse v1 contract or
  add a stake-management satellite contract — design decision deferred to
  v2.

The bench crate (`bench/`) measures host primitives independently of any
contract version and remains stable across the v0 → v1 → v2 transition.

---

## 12. Open design questions

These are deliberately unresolved and will be answered during v1–v2 based on
measurements and conversations with publishers / consumers:

1. **BLS aggregate vs. parallel Ed25519 at 5+ publishers.** At what publisher
   count does the BLS aggregate path become cheaper than parallel Ed25519
   verify? Bench data exists for both; the threshold needs to be computed
   end-to-end including tx-size effects.
2. **Confidence intervals.** Pyth exposes a per-feed confidence interval.
   Whether to compute one client-side (from the source spread) or accept it
   as a publisher-attested field is a v2 decision tied to RWA primitives.
3. **TWAP exposure.** TWAPs are valuable for AMM oracle-priced pools but are
   awkward in a pull-only model (a single signed point doesn't give a window).
   Either signed TWAPs as a separate attestation type (publisher computes off
   the spot stream) or consumer-side TWAP from a window of signed spots.
   A v2 decision. The Feb 2026 Blend / YieldBlox exploit (~$10.8M) is the
   cautionary case: lending-style consumers need manipulation-resistant
   valuation, not just freshness.
4. **Cross-asset routing.** Should the contract support price quotes between
   two non-USD assets natively, or should consumers compose two USD-denominated
   reads? Affects AMM use cases.
5. **Publisher onboarding contract.** Whether to keep `set_publishers` as a
   simple admin call (v0–v1) or to gate via a stake-locked join transaction
   (v2). Affects whether onboarding is permissioned or permissionless.

---

## 13. References

- [`oracle_v0/src/lib.rs`](oracle_v0/src/lib.rs) — production contract source.
- [`bench/src/test.rs`](bench/src/test.rs) — host-isolated cost measurement
  harness; reproducible via `cargo test -p noeracle_bench -- --nocapture`.
- [`scripts/run_oracle_bench.mjs`](scripts/run_oracle_bench.mjs) — end-to-end
  fee measurement against real testnet / mainnet.
- [`scripts/fetch_ledger_limits.mjs`](scripts/fetch_ledger_limits.mjs) — live
  `ConfigSettingEntry` dump for current network limits.
- [`DOCUMENTATION_PLAN.md`](DOCUMENTATION_PLAN.md) — project documentation
  blueprint and phasing.
- [`proposals/sdf-hackathon-istanbul-2026/MEMO.md`](proposals/sdf-hackathon-istanbul-2026/MEMO.md)
  — SDF resource-listing request for the Istanbul hackathon.
