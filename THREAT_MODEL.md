# Noeracle — STRIDE Threat Model

**Prepared for:** Soroban Security Audit Bank submission (follows the SDF STRIDE template: *what are we working on / what can go wrong / what are we going to do about it / did we do a good job*)
**Project:** Noeracle — pull-based price oracle on Soroban (`github.com/noeracle/noeracle`)
**Date:** 2026-08-23 · **Version:** 1.1 (v1.0 plus same-day remediation of the 2026-08-23 Almanax scan; supersedes the v0-era page at docs.noeracle.org/concepts/threat-model)
**Companions:** `ARCHITECTURE.md`, `REVIEW-L08-L09.md` (internal review of the quorum + history-ring hardening), `audit/` (Scout + cargo-audit reports), `scripts/keeper/README.md`

**Relationship to Noether.** Noeracle is the sole price source of Noether, the perpetual futures DEX built by the same team (SCF #41, separate Audit Bank application). Noether's router relays a fresh Noeracle attestation and calls its market in the same transaction, so Noether's price integrity reduces entirely to the properties in this document. The two should be reviewed against each other at the router ↔ oracle boundary.

---

## 1. What are we working on?

Noeracle is a **pull oracle**. An off-chain attestation service polls five exchanges every ~500 ms, aggregates a per-asset price, and signs `(asset, price, timestamp, round_id)` with a registered Ed25519 publisher key. A consumer fetches the freshest signed attestation and bundles the verification call into **its own** transaction, so its application logic executes against a price signed within the last second rather than against pre-warmed on-chain state. Relaying is permissionless: authority lives in the signature, never in the caller. Fifteen USD pairs are served today on Stellar testnet.

The on-chain component is one Soroban contract, `oracle_v0` (~757 lines, 16 KB WASM), exposing eleven entrypoints plus a deploy-time constructor on the live instance (verified on-chain 2026-08-23): `__constructor` (runs only at deploy), `set_publishers`, `set_quorum`, `get_quorum`, `upgrade`, `update_batch_ed25519_args`, `update_batch_ed25519_persistent`, `update_quorum_ed25519_persistent`, `get_price`, `get_price_pers`, `prices`, `twap`. Scope details, deployed addresses and the freeze protocol are in Appendix A.

### 1.1 Flow (the "happy path")

1. **Poll.** The attestation service sends one batched spot-ticker request per exchange (Binance, Coinbase, OKX, Kraken, Bybit) every 500 ms.
2. **Aggregate.** Samples older than 5 s are dropped; with at least three sources, samples beyond 3σ are dropped; the remainder is averaged with fixed weights (Binance 3, Coinbase 2, OKX 2, Kraken 1, Bybit 1). USDT quotes are treated at USD parity.
3. **Sign.** `round_id = floor(unix_ms / 500)`. For each asset the 40-byte message `asset(8) ‖ price(i128 BE) ‖ timestamp(u64 BE) ‖ round_id(u64 BE)` is Ed25519-signed with the publisher key (held as a Container Apps secret on Azure, single replica).
4. **Serve.** Attestations are served over HTTPS and SSE at `api.noeracle.org`. The TypeScript SDK rejects anything older than 2 s (`StalePriceError`).
5. **Relay.** A consumer (Noether's router or keeper, or any SDK user) submits the attestation inside its own transaction: `update_batch_ed25519_persistent` for readers between heartbeats (Noether), `update_batch_ed25519_args` for verify-then-read-inline consumers, or `update_quorum_ed25519_persistent` carrying several publishers' rounds when quorum > 1.
6. **Verify and store.** The contract checks vector lengths, the quorum gate, membership of the signing key in the admin-maintained publisher set, price bounds (0 < p ≤ 1e30), a 60 s staleness bound against ledger time plus a 30 s future-skew allowance, every per-asset signature via the host's Ed25519, and a per-asset monotonic `round_id`; only a newer round is written. Persistent writes extend the entry's TTL and append to a 32-entry per-asset history ring. On the quorum path the stored value is the per-asset **median** of the submitted publisher prices.
7. **Read.** Consumers read `get_price` (temporary) or `get_price_pers` (persistent), plus `prices`/`twap` over the ring. Noether reads through a SEP-40 shim and applies its own staleness bound and deviation band against its last-good price.
8. **Administer.** A single admin account can `init` (once), `set_publishers`, `set_quorum`, and `upgrade` the WASM.

### 1.2 Data-flow diagram

```
 EXTERNAL ENTITIES            PROCESSES (team-controlled)                 DATA STORES
 ─────────────────            ───────────────────────────                 ───────────
 ┌─────────────┐   (1) spot   ┌──────────────────────────┐
 │ 5 exchanges │ ───────────► │ attestation service      │   publisher secret (Azure secret)
 └─────────────┘   tickers    │ (2) aggregate (3) sign   │
                              └────────────┬─────────────┘
                                           │ (4) HTTPS / SSE: signed attestations
 ┌───────────────────────────┐             ▼
 │ consumer / relayer        │ ◄───────────┘
 │ (SDK user, Noether router │
 │  or keeper — untrusted)   │
 └────────────┬──────────────┘
              │ (5) relays attestation inside its own Stellar transaction
═════════════ ▼ ══════ TRUST BOUNDARY: publisher registry + host Ed25519 verify ══════
 ┌───────────────────────────────────────────────────────────────────────────────┐
 │ oracle_v0 (Soroban)                                                            │
 │ (6) lengths → quorum gate → registry → staleness ≤ 60 s → sig/asset           │
 │     → monotonic round → write + TTL → ring                                     │
 │        ┌──────────────┐  ┌──────────────┐  ┌─────────────┐  ┌───────────────┐ │
 │        │ PriceTemp    │  │ PricePers    │  │ PriceRing   │  │ Admin /       │ │
 │        │ (temporary)  │  │ (persistent) │  │ (32, pers.) │  │ Publishers /  │ │
 │        └──────────────┘  └──────────────┘  └─────────────┘  │ Quorum (inst.)│ │
 │                                                             └───────────────┘ │
 └───────────────────────────────────────────────────────────────────────────────┘
              ▲ (7) get_price / get_price_pers / prices / twap        ▲ (8) admin ops
 ┌────────────┴──────────────┐                            ┌───────────┴───────────┐
 │ readers (Noether shim →   │                            │ admin (single Stellar │
 │ market: own bands + 60 s) │                            │ account, no timelock) │
 └───────────────────────────┘                            └───────────────────────┘
```

Everything above the trust boundary is untrusted transport: compromising it yields *rejected* attestations (signatures fail on-chain), never accepted bad prices. Only the publisher key and the admin key sit inside the boundary.

### 1.3 Actors and trust

| Actor | Trust | Can | Cannot |
|---|---|---|---|
| Publisher (today one self-operated key) | trusted for price integrity | sign price messages | write on-chain directly; bypass staleness or monotonic checks |
| Relayer / consumer (anyone) | untrusted | submit publisher-signed attestations; read | forge or alter a price; regress a stored round; use an unregistered key |
| Attestation-service operator (the team) | trusted (holds the publisher secret and aggregation code) | choose sources, weights, policy; rotate keys with the admin | — (operator compromise ≡ publisher compromise) |
| Exchanges | untrusted data sources | feed spot prices | individually move the aggregate beyond weight and outlier limits |
| Admin (single account) | trusted — highest residual risk | constructor (at deploy), `set_publishers`, `set_quorum`, `upgrade` | write prices; bypass signature verification |

---

## 2. What can go wrong?

### 2.1 STRIDE reminders

| Threat | Definition | Question |
|---|---|---|
| **S**poofing | Impersonating another user or system component | Is the signer / caller who they claim to be? |
| **T**ampering | Unauthorized alteration of data or code | Has the price, round, or contract been modified? |
| **R**epudiation | Denying having taken an action | Can we prove who produced a stored price? |
| **I**nformation disclosure | Over-sharing data expected to be private | Is anything private exposed? |
| **D**enial of service | Negatively affecting availability | Can someone stop prices from being fresh or readable? |
| **E**levation of privilege | Gaining roles beyond those granted | Can someone gain publisher or admin power? |

### 2.2 Threat table

| Threat | Issues |
|---|---|
| **S**poofing | **Spoof.1** (Step 5–6) An attacker relays an attestation signed by a key that is not a registered publisher, or with a forged signature. **Spoof.2** (Step 8) On a fresh deployment, an attacker claims the instance in a deploy→initialize window and installs themselves as admin and publisher (raised as Almanax ALX-1). **Spoof.3** (Step 6, quorum path) One publisher's round is submitted twice to satisfy an M-of-N quorum with fewer than M real publishers. **Spoof.4** (Step 4) A look-alike attestation endpoint (DNS or TLS compromise) feeds consumers fabricated attestations. |
| **T**ampering | **Tamper.1** (Step 5) A relayer alters the price or timestamp of an attestation in transit. **Tamper.2** (Step 5–6) An old signed round is replayed to regress a fresher stored price. **Tamper.3** (Step 6) With quorum > 1, one compromised registered key uses the single-publisher persistent path to set the stored price outright, bypassing the median. **Tamper.4** (Step 1–2) An exchange feed is manipulated (thin market, wash trade, or bad data) so the signed aggregate is wrong. **Tamper.5** (Step 5) A valid attestation for one instance is replayed into another instance or network that registers the same publisher key — the signed message carries no network, contract-id, or scheme domain separator. **Tamper.6** (Step 6) A signature produced for one asset is submitted under another asset. **Tamper.7** (Step 8) A malicious or erroneous `upgrade` replaces the verification logic. **Tamper.8** (Step 6) A publisher-signed FUTURE-dated timestamp reads as age 0 under saturating subtraction and is accepted as fresh (Almanax ALX-3). **Tamper.9** (Step 6) Extreme publisher-signed price values could overflow the `twap` ring sum and trap the view (Almanax ALX-4). |
| **R**epudiation | **Repudiate.1** (Step 6) The contract emits no events, so reconstructing *which* round was accepted and *when* requires ledger-entry archaeology rather than an event log. **Repudiate.2** (Step 3) The publisher denies having signed a bad price. |
| **I**nformation disclosure | **Info.1** (Steps 3, 8) Exposure of the publisher secret or the admin secret — the only private data in the system; prices themselves are public by design. **Info.2** (Step 4) Service metadata (`/health`, source counts, publisher public key) reveals operational state. |
| **D**enial of service | **DoS.1** (Step 1) Exchanges rate-limit or IP-ban the service's egress (this happened on 2026-06-02 and took the service offline). **DoS.2** (Step 4) The attestation service — a single operator and single replica — goes down. **DoS.3** (Step 6) Persistent entries archive if no write extends their TTL for ~3.5–7 days; readers then get `None`. **DoS.4** (Step 8) The admin shrinks the publisher set below the configured quorum, making the quorum path unmeetable. **DoS.5** (Step 5–6) Oversized batches or many assets in one call exhaust transaction resources. |
| **E**levation of privilege | **Elevation.1** (Step 8) Admin key compromise: full control of the publisher set, quorum, and contract code, with no timelock. **Elevation.2** (Step 3) Publisher key compromise: at quorum 1 the attacker can sign any price for any asset. **Elevation.3** (Step 2–3) Operator or hosting compromise (cloud account, deploy pipeline, host) — equivalent to Elevation.2. |

---

## 3. What are we going to do about it?

| Threat | Issues and treatments |
|---|---|
| **S**poofing | **Spoof.1** — *S1R1 (implemented):* every write path checks the signing key against the admin-maintained publisher set (`UnknownPublisher`) and verifies each per-asset signature with `env.crypto().ed25519_verify`; a failed verify traps the whole transaction. **Spoof.2** — *S2R1 (implemented 2026-08-23, ALX-1):* the separate `init` entrypoint was REMOVED; a Soroban `__constructor` initializes admin and publisher set atomically inside the deploy transaction, leaving no claimable window, and still requires the admin's authorization so a deployer cannot install someone else. *S2R2 (process):* the deploy script passes constructor arguments to `stellar contract deploy` and its interface gate rejects any wasm still exporting `init`. **Spoof.3** — *S3R1 (implemented):* the quorum path rejects a repeated key (`DuplicatePublisher`) and counts distinct registered publishers only. **Spoof.4** — *S4R1 (implemented):* fabricated attestations fail on-chain regardless of transport, so the impact is availability, not integrity; TLS on `api.noeracle.org`, Cloudflare-managed DNS. *S4R2 (planned):* SDK option to pin the expected publisher public key client-side so a bad endpoint fails fast. |
| **T**ampering | **Tamper.1** — *T1R1 (implemented):* the signature covers price, timestamp and round; any alteration fails verification. **Tamper.2** — *T2R1 (implemented):* rounds older than 60 s are rejected (`StalePrice`); *T2R2 (implemented):* per-asset monotonic `round_id` — a lagging or replayed round is a silent no-op (deliberate: a consumer's transaction must never fail because someone else's fresher round landed first). **Tamper.3** — *T3R1 (implemented, closed in `fcec0ee`):* `update_batch_ed25519_persistent` returns `QuorumNotMet` whenever quorum > 1, so the only persistent writer above quorum 1 is the median path; tests pin both directions. **Tamper.4** — *T4R1 (implemented):* 5 s sample cut-off, 3σ outlier rejection with ≥ 3 sources, fixed exchange weights; *T4R2 (planned):* asset-inclusion policy (minimum venues and depth) for thin assets; *T4R3 (consumer-side, implemented in Noether):* router publisher allowlist + coarse per-asset bands, then market-side deviation band against last-good price. **Tamper.5** — *T5R1 (process):* distinct publisher keys per network and per environment so a testnet attestation is never valid on mainnet; *T5R2 (proposed for v1, auditor input requested):* add a domain separator (network passphrase hash + contract id + scheme version) to the signed message. **Tamper.6** — *T6R1 (implemented):* the 8-byte asset tag is the first field of the signed message. **Tamper.7** — *T7R1 (implemented):* `upgrade` is admin-authenticated; *T7R2 (process):* builds are reproducible from a tagged commit and the uploaded WASM hash is verified before `upgrade` is invoked; *T7R3 (planned):* admin under a multisig with a timelock (see Elevation.1). **Tamper.8** — *T8R1 (implemented 2026-08-23, ALX-3):* every write path also rejects `timestamp > now + 30 s`; the allowance covers keeper-vs-ledger clock skew, tested on all three paths. **Tamper.9** — *T9R1 (implemented 2026-08-23, ALX-4):* prices are bounded at write time (`0 < price ≤ 1e30`, `InvalidPrice`), making a 32-entry ring sum arithmetically unable to overflow; the `twap` unwrap became a `None` belt, and consumers can never read a zero or negative price. |
| **R**epudiation | **Repudiate.1** — *R1R1 (accepted for now, auditor input requested):* no events is a deliberate fee choice; ledger entry changes are archived by Stellar history and the Noether indexer; *R1R2 (proposed):* emit `updated(asset, round_id, price)` if the auditors consider it worth its cost. **Repudiate.2** — *R2R1 (implemented):* every stored entry was admitted by an Ed25519 signature over exactly the stored fields, attributable to a registered key; the service logs each signed round with its `round_id`. |
| **I**nformation disclosure | **Info.1** — *I1R1 (implemented):* the publisher secret lives only as a Container Apps secret and is never logged; `/health` exposes the public key only; the admin secret is held offline in the team's CLI keystore and never on a server. *I1R2 (planned):* multisig admin (see Elevation.1). **Info.2** — *I2R1 (accepted):* operational metadata is intentionally public; it carries no secrets. |
| **D**enial of service | **DoS.1** — *D1R1 (implemented):* one batched request per venue (~10 req/s per machine regardless of asset count — per-symbol polling was the cause of the June ban); *D1R2 (implemented):* tolerance to losing venues (aggregation proceeds with the sources that respond); *D1R3 (implemented):* `/health` serves the last signed price up to 60 s stale and returns 503 beyond that, a watchdog restarts the process, and an external monitor pages Telegram every 10 minutes. **DoS.2** — *D2R1 (implemented):* every layer fails closed — the SDK raises `StalePriceError`, the contract rejects `StalePrice`, Noether surfaces staleness rather than trading on stale data; *D2R2 (planned):* re-establish multi-region replicas on the current host. **DoS.3** — *D3R1 (implemented):* every persistent write extends TTL (threshold ~3.5 days, extend ~7 days) and the Noether keeper runs a periodic TTL-bump duty over every consumed contract; *D3R2 (implemented):* archived entries read as `None` — a liveness failure, never a wrong price. **DoS.4** — *D4R1 (accepted, documented):* only the admin can cause it, and `set_quorum` re-validates `1 ≤ quorum ≤ publishers.len()` at set time. **DoS.5** — *D5R1 (implemented):* the caller pays the verification cost of its own batch; no shared state amplifies it; the history ring is capped at 32 entries. |
| **E**levation of privilege | **Elevation.1** — *E1R1 (planned, hard mainnet gate):* admin role moved to a multisig distinct from the publisher set, with a timelock on `upgrade` and `set_publishers`; *E1R2 (implemented):* admin secret kept offline, used only in ceremonies. **Elevation.2** — *E2R1 (implemented):* the M-of-N median path (`update_quorum_ed25519_persistent`) is deployed and tested: once armed, one compromised key moves the stored price by at most one rank; *E2R2 (planned, hard mainnet gate):* register independent publishers and raise the quorum — deliberately descoped on 2026-07-21 until such publishers exist, so the live instance runs at quorum 1; *E2R3 (consumer-side, implemented):* Noether's bands cap per-update damage. **Elevation.3** — *E3R1 (implemented):* single-owner cloud subscription with MFA, secrets in the Container Apps secret store, deploys only through a scripted path; *E3R2 (planned):* publisher key rotation runbook and a second, independently operated publisher. |

---

## 4. Did we do a good job?

- **Has the data-flow diagram been referenced since it was created?** Yes — the eight-step flow is the frame both for this table and for Noether's threat model, which treats Step 5–7 as its own "price relay" boundary.
- **Did the STRIDE model uncover new design issues?** Yes, two. The absence of a domain separator in the signed message (Tamper.5) had not been written down anywhere before this exercise; and framing "no events" as a repudiation question (Repudiate.1) turned an implicit cost choice into an explicit decision we now want an auditor's view on. The single-key quorum bypass (Tamper.3) was found by the earlier internal review and fixed before this model was written.
- **Do the treatments adequately address the issues?** For the current testnet scope, yes. For mainnet, two items are explicitly *not* mitigated today and are declared hard gates: a single publisher key (Elevation.2) and a single admin key without a timelock (Elevation.1). We would rather the auditors evaluate the designed fixes than merely confirm the gaps.
- **Have additional issues been found after the threat model?** Yes — an Almanax scan two days after v1.0 raised four findings: the deploy→init race (real; fixed by replacing `init` with a deploy-time constructor), the missing domain separator (already §N-4 — independent corroboration), a future-timestamp staleness bypass, and a `twap` overflow trap (both fixed same day). All three fixes shipped to both live instances through `upgrade()` within hours — the living-document loop working as intended (this is v1.1).
- **Insights for next time:** the most valuable questions were the cross-boundary ones (what does a valid signature mean *outside* the instance it was meant for), not the per-function ones.

---

## Appendix A — Audit scope

| Crate / component | Deployed | Role | Functional LOC (excl. tests) | Tests |
|---|---|---|---|---|
| `oracle_v0` (`noeracle_oracle_v0`) | ✔ | Signed-price verification, storage, quorum median, history ring | 757 in `src/lib.rs` (~150 of these are feature-gated benchmark entrypoints absent from production builds) | 46 |
| `bench` | ✗ | Host-cost harness for signature schemes | excluded | 8 |
| `scripts/keeper/` (Node) | off-chain | Attestation service | adjacent | 22 |
| `sdk/` (`@noeracle/sdk`) | off-chain | Consumer integration | adjacent | — |

Workspace: `soroban-sdk` 26, target `wasm32v1-none`, release profile `overflow-checks = true`, `panic = "abort"`, `lto`. Production WASM 16,084 bytes.

**Excluded from the production surface:** six `update_*` benchmark entrypoints (`update_ed25519_args/stored/persistent`, `update_bls_agg`, `update_secp256k1`, `update_via_auth`) perform no publisher, staleness, or monotonicity checks and compile only under the `bench` cargo feature (off by default). The live instance's function list confirms they are absent.

**Deployed instances (Stellar testnet):** production `CBTO5K2NLG2KYHQDL5ME4SWFQ5GRR7GVU4DFATOXGVS3OUJJDFF2YYNS` (consumed by Noether's production and public-testnet stacks), staging `CB2D6BDZALAMOZJMOA66EJPTNK55YCTWJ5DJDUKXE6G7XJK734GTSW5N`. The pre-hardening instance `CAYIP67…` is retired.

**Freeze protocol.** The audited surface is the `oracle_v0` crate at the commit the live instance was built from (`8ee5e59`, branch `feat/l08-l09-quorum-ring`, merged to `main`). The remediated build was upgraded in place onto BOTH live instances on 2026-08-23 and hash-verified byte-equal (`f9bd9b11…`). It will be tagged; nothing lands on the tag during the engagement. Future changes ship through `upgrade()` from a tagged, hash-verified build.

## Appendix B — The price-integrity chain, ranked

**Invariant:** *every stored price was signed by a registered publisher at most 60 s before it was written, is newer than what it replaced for that asset, and — whenever quorum > 1 — is the median of at least `quorum` distinct registered publishers' prices for one synchronized round.*

1. **Publisher key compromise** (Elevation.2) — at quorum 1, full oracle compromise bounded only by consumer-side bands.
2. **Operator / host compromise** (Elevation.3) — equivalent to 1.
3. **Source manipulation** on thin markets (Tamper.4) — requires moving the weighted majority of five venues inside one 500 ms round; HYPE and ZEC are the thinnest assets served.
4. **Staleness and halt** (DoS.1–3) — fail-closed at every layer.
5. **Cross-instance replay** (Tamper.5) — consistency, not forgery.

**Questions we would most like answered:** Is median-of-rounds sound against a colluding minority plus one honest-but-stale publisher? Should the signed message gain a domain separator before mainnet, and is a 60 s staleness bound right for a 5 s ledger? Is the `twap()` view (mean over stored rounds, not time-weighted) safe to expose to lending-style consumers, or should it be renamed or withheld until a signed-TWAP attestation type exists?

## Appendix C — Residual risks gating mainnet

| # | Risk | Status |
|---|---|---|
| N-1 | Single publisher key, quorum 1 | M-of-N path deployed and tested; arming deferred until independent publishers exist. **Hard gate.** |
| N-2 | Single admin account, no timelock | Multisig migration planned alongside Noether's key ceremony. **Hard gate.** |
| N-3 | Single attestation operator and replica | Liveness only; multi-region replicas planned. |
| N-4 | No domain separation in the signed message (independently corroborated by Almanax ALX-2) | Distinct keys per network now; domain tag proposed for v1. Auditor input requested. |
| N-5 | No on-chain events | Cost choice; auditor input requested. |
| N-6 | Persistent entries depend on continued writes for rent | Fail-closed plus consumer-side TTL bump duty. |
| N-7 | USDT ≈ USD parity assumed; no confidence interval; `twap()` is round-weighted | Documented; v2 items. |

## Appendix D — Security practices to date

- **Tests:** 46 contract tests (init authorization, unknown and duplicate publishers, batch-length mismatches, staleness, monotonic no-op on lagging and replayed rounds, quorum threshold and median, single-path auto-close at quorum > 1 and reopen at 1, history ring and views, `upgrade`); 8 host-cost harness tests with committed ledger snapshots; 22 aggregation tests on the attestation service.
- **Build hardening:** `overflow-checks = true`, `panic = "abort"`; checked arithmetic in `median` and `twap`, saturating arithmetic in the staleness check.
- **Feature gating:** benchmark entrypoints compiled out of production builds; live function list verified after deploy.
- **Static analysis and advisories:** `cargo-scout-audit` 0.3.16 — 0 detections on both crates; `cargo audit` — clean (yanked-crate warnings only); **Almanax** (Stellar agent) — 4 findings, 3 remediated and shipped the same day, 1 = the pre-disclosed §N-4. Reports and triage in `audit/`.
- **CI:** contract tests on every push; external keeper monitor and SLA-check workflows.
- **Internal review:** `REVIEW-L08-L09.md` found and closed the single-publisher quorum bypass before deployment.
- **Operations:** reproducible deploy + init script; documented `/health` semantics; Telegram paging.

## Appendix E — Assumptions and out of scope

**Assumed sound:** the Soroban host's Ed25519 verification and ledger timestamp; Stellar consensus; TLS to the exchanges' public APIs and to `api.noeracle.org`.

**Out of scope for this engagement:** the `bench` crate and all `bench`-feature entrypoints (never deployed), the landing and documentation sites, and the TypeScript SDK (no trust role). The attestation service is described for completeness; a review of its aggregation policy is welcome if the firm's scope allows, but the contract is the audited artifact.
