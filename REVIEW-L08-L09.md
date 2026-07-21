# Founder review brief — `feat/l08-l09-quorum-ring`

**What this is:** the L0-8 (publisher quorum + on-chain median) and L0-9 (price
history ring + `prices`/`twap` views) hardening of `oracle_v0`, plus the
`upgrade()` entrypoint. It is the deploy source for the Batch-1 fresh Noeracle
on the Noether side. 6 commits, all inside `oracle_v0/src/{lib,test}.rs` —
**275 insertions of contract code, 41 tests green** (was 18 on main).

Everything else in this file exists to make the review take ~30 minutes
instead of an evening. Read the diff with:

```bash
git diff main..feat/l08-l09-quorum-ring -- oracle_v0/src/lib.rs
```

---

## The six commits

| Commit | What it does |
|---|---|
| `6436bbf` | `upgrade(new_wasm_hash)` — admin-gated WASM swap. Deliberately makes THIS the last forced redeploy; future changes land in place. |
| `b69638e` | `set_quorum(m)` / `get_quorum()` — instance-stored threshold, default **1** when never set (existing deployments unchanged). Validated `1 <= m <= publishers.len()` at set time. |
| `8499555` | Per-asset history ring (`RING_CAP = 32`) fed by **every** persistent write, plus `prices(asset, records)` (latest-first) and `twap(asset, records)` (mean over last k rounds) views. |
| `6e9ef13` | `update_quorum_ed25519_persistent(assets, timestamp, round_id, rounds)` — the M-of-N median path (details below). |
| `8fe3c3b` | Snapshot refresh. |
| `fcec0ee` | **Closes the bypass**: `update_batch_ed25519_persistent` now rejects with `QuorumNotMet` while quorum > 1 (see "The one real finding" below). |

New errors: `InvalidQuorum = 6`, `DuplicatePublisher = 7`, `QuorumNotMet = 8`.
New storage: `DataKey::Quorum` (instance), `DataKey::PriceRing(asset)`
(persistent, same TTL policy as `PricePers`).

## How the quorum path works (the part worth reading closely)

One relayer submits ONE synchronized round: every publisher signed the same
`(assets, timestamp, round_id)` over its own prices. The contract:

1. rejects misaligned rounds (`prices.len() != assets.len()` etc.) — `BatchLengthMismatch`;
2. rejects rounds older than `STALENESS_SECS` (60s, unchanged);
3. requires every `pubkey` registered, each at most once (`UnknownPublisher` / `DuplicatePublisher`);
4. requires at least `quorum()` rounds (`QuorumNotMet`);
5. verifies **every** signature — per publisher per asset, `build_msg` **verbatim**
   (the same 40-byte `asset‖price‖ts‖round` layout the single-publisher paths
   and the JS golden vectors pin, so the off-chain signing service is
   unchanged);
6. stores the per-asset **median** behind the same newer-round-only check as
   the existing persistent path (a lagging round is a silent no-op), and the
   stored entry feeds the history ring.

Rounds are call data only — nothing per-publisher is ever stored.

## The one real finding (already fixed on the branch)

As merged through `6e9ef13`, raising the quorum did **not** actually enforce
M-of-N: `update_batch_ed25519_persistent` (single publisher) writes the same
`PricePers` slots, so one compromised registered key could keep setting
prices outright and bypass the median. `fcec0ee` closes it — the batch path
returns `QuorumNotMet` whenever quorum > 1, and a test pins both directions
(closed at quorum 2, reopened at quorum 1). At the default quorum of 1
nothing changes, which is exactly the staged rollout: Batch-1 deploys at
quorum 1; the day you arm quorum ≥ 2, the legacy path closes by itself and
the keeper must publish via the quorum entrypoint.

Note the temporary-storage paths (`update_ed25519`, `update_batch_ed25519`)
stay single-key at any quorum. That's deliberate: temp feeds only the
display read (`get_price`); the market/shim read `get_price_pers`. Confirm
you're comfortable with that split.

## What I'd eyeball, in order

1. **`update_quorum_ed25519_persistent`** top to bottom (~100 lines) — check
   the validation ORDER yourself: alignment → staleness → registration/dupes
   → quorum count → signatures → median store. Anything that returns early
   must leave no partial writes (it can't — all writes happen in the final
   loop).
2. **`median()`** — insertion sort, odd takes middle, even takes mean of
   middles with a checked add. Prices are 1e7-scaled USD so overflow is
   unreachable in practice; the checked add traps rather than wraps on
   absurd inputs.
3. **`fcec0ee`'s guard placement** — first thing after arg-shape validation
   in the batch path, before any signature work.
4. **`push_ring`** — lives inside `write_pers`, so both persistent writers
   feed history and only rounds that passed the monotonic check land. Ring
   evicts oldest at 32 (`remove(0)`, O(32), fine).
5. **`set_quorum` / `upgrade` auth** — both `admin.require_auth()` gated,
   same pattern as `set_publishers`. `upgrade` has no timelock — consistent
   with the current single-admin model; the multisig migration covers it
   later.

## Sharp edges to sign off on (accepted, not bugs)

- **Even-count medians are means.** At quorum 2 with exactly 2 rounds, one
  compromised key drags the stored price to the midpoint between honest and
  attacker values — HALF-weight, not one-rank-bounded. The "moves it at most
  one rank" property holds for odd submission counts. Practical arming
  guidance: target **3 submitted rounds** (2-of-3 means m ≥ 2, but the
  keeper should submit all 3 when healthy), and remember Noether's own
  router bands + market deviation band (#81) cap the damage regardless.
- **`set_publishers` can shrink below quorum.** Then the quorum path can't
  meet quorum until the admin re-syncs (documented in the doc comment).
  Fail-closed, so safe — but it's an operational footgun to know about.
- **Verify cost scales M×N.** 3 publishers × 14 assets = 42 ed25519 verifies
  in one tx. Fine at Batch-1 (the router relays 1×1; the keeper batches
  1×14). Before arming quorum ≥ 2 with full-asset keeper submissions, run
  the `bench` crate against the current host budget once.
- **`twap` is a mean over ROUNDS, not time-weighted** — stated in its doc
  comment. The Noether shim/market treat it as a smoothing eligibility
  signal only; settlement never uses it.
- **No events** on any write path — consistent with the existing contract.

## Test coverage (41 green)

18 pre-existing (golden vectors, batch, staleness, rotation) + new suites:
quorum default/threshold/dupes/unknown/misalignment/staleness, median odd /
even / per-asset independence, one-bad-signature-reverts-everything, lagging
round is a silent no-op (no ring entry), ring caps at 32 latest-first, twap
mean + windowing, `set_quorum` bounds + auth, `upgrade` auth, and the
`fcec0ee` batch-path closure both ways.

## What happens after your approval

Fresh deploy from this branch (no `upgrade()` exists on the old instance —
this is the last forced redeploy), publisher key registered at init,
`set_quorum(1)`, then the Noether Batch-1 script points shim/router/keeper
at it. Quorum ≥ 2 arming is a later, separate step gated on more publisher
keys + the keeper multi-key publish loop.
