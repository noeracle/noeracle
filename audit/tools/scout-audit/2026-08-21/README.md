# Scout Audit Scan — 2026-08-21

> **Type: tool report (static analysis / lints). Not an audit.**

| | |
|---|---|
| Tool | CoinFabrik `cargo-scout-audit` **0.3.16** (Soroban detector suite) |
| Target | `noeracle/noeracle` @ `c9385b1` (branch `feat/l08-l09-quorum-ring` — the deploy source of the live testnet instance) |
| Scope | Cargo workspace: `noeracle_oracle_v0` (deployed contract) + `noeracle_bench` (cost harness, not deployed) |
| Result | **0 detections** on both crates (Critical 0 / Medium 0 / Minor 0 / Enhancement 0) |
| Raw output | `scout-report.md`, `scout-report.json` |

**Reading the zero.** The contract is ~570 production lines with no admin
setters beyond `set_publishers` / `set_quorum` / `upgrade` (all
`require_auth`-gated inline, which Scout recognizes), no unchecked arithmetic
on user input (`checked_add` in `median` and `twap`, `saturating_sub` on the
staleness check), no unbounded storage growth (history ring capped at 32),
and `overflow-checks = true` in the release profile. Scout's Soroban
detectors therefore have nothing to flag. A clean tool run is a floor, not a
ceiling — the items we actually want an auditor's eyes on are listed in
`THREAT_MODEL.md` §4 and §6 (single-publisher quorum, missing domain
separator in the signed message, no events, TTL-by-writes).

Companion: `../../cargo-audit/2026-08-21/report.txt` (advisory scan —
clean; warnings only for yanked transitive crates).
