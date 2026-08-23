# Almanax Scan — 2026-08-23 (triage + remediation)

> **Type: tool report (AI static analysis). Not an audit.**

| | |
|---|---|
| Tool | Almanax (app.almanax.ai), Stellar agent, Default scan mode |
| Target | `noeracle/noeracle` @ `main` (`b30f1d0`), scope `oracle_v0/` only |
| Result | **4 findings**: 1 Critical, 2 Medium, 1 Low |
| Triage | Verified against source same day; **3 remediated in `8ee5e59`**, 1 pre-disclosed design question deferred with rationale |
| Raw findings | `findings.md` (as exported from the dashboard) |
| Shipped | The fix commit was built, tested (46 contract tests green) and **upgraded in place onto both live instances** (staging `CB2D6BDZ…`, production `CBTO5K2N…`) via the admin-gated `upgrade()` on 2026-08-23; deployed code hash-verified against the build. |

## Verdicts

**ALX-1 · init front-run (Critical) — CONFIRMED, fixed.**
Real: `admin.require_auth()` stops installing SOMEONE ELSE as admin, but any
caller could name THEMSELVES in the deploy→init window (the deploy script ran
them as two transactions). Not exploitable retroactively — both live instances
were long initialized — but a live risk for any future (mainnet) deploy.
Fix (`8ee5e59`): `init` REMOVED; a Soroban `__constructor` now initializes
admin + publisher set atomically inside the deploy transaction; the deploy
script passes constructor args to `stellar contract deploy` and its interface
gate requires `__constructor` and rejects any wasm still exporting `init`.
The threat model's earlier "cannot be claimed by whoever calls init first"
wording was WRONG and has been corrected.

**ALX-2 · no domain separation in the signed message (Medium) — CONFIRMED,
deliberately deferred.** Already disclosed pre-scan as threat-model N-4
(Tamper.5) with auditor input requested; Almanax independently corroborates.
Fixing changes the 40-byte signed format and forces a coordinated cutover of
the attestation service, the SDK, the Noether keeper + router, and the pinned
golden vectors — the planned v1 attestation restructure. Present-day impact
is nil: both instances share one honest key signing identical content, so a
cross-instance replay replays the same price. Mainnet uses distinct keys per
network regardless.

**ALX-3 · future timestamps bypass staleness (Medium) — CONFIRMED, fixed.**
`saturating_sub` read far-future stamps as age 0. Exploitation requires a
publisher-signed message (a compromised key could sign any PRICE anyway), so
low in our trust model — but cheap. Fix (`8ee5e59`): every write path also
rejects `timestamp > now + 30s`; the 30 s skew allowance keeps the happy path
robust to keeper-vs-ledger clock drift. Tests on all three paths.

**ALX-4 · twap overflow trap (Low) — CONFIRMED as theoretical, fixed at the
root.** Fix (`8ee5e59`): prices are bounded at write time (`0 < price ≤ 1e30`,
new `InvalidPrice` error, checked on all three paths incl. per-round in the
quorum path) — which makes a 32-entry ring sum arithmetically unable to
overflow — plus the twap `unwrap` became a `None` belt. Bonus hardening:
consumers can now never read a zero or negative price from this oracle.
