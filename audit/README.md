# Noeracle — security artifacts

Self-administered checks performed ahead of the Soroban Security Audit Bank
engagement. Tool output is evidence, not an audit; the threat model lives at
the repo root (`THREAT_MODEL.md`).

| Date | Tool | Result | Path |
|---|---|---|---|
| 2026-08-21 | cargo-scout-audit 0.3.16 | 0 detections (both crates) | `tools/scout-audit/2026-08-21/` |
| 2026-08-21 | cargo-audit | clean (yanked-crate warnings only) | `tools/cargo-audit/2026-08-21/` |
