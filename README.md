# Noeracle

An on-demand (pull-based) price oracle for Stellar / Soroban — designed to
complement Reflector's push-based oracle.

A consumer fetches a freshly signed price attestation off-chain and bundles a
verification operation into its own Stellar transaction. The Soroban contract
verifies the publisher signature, checks freshness, and stores the price, so
the consumer's application logic executes against a price signed sub-second
before the transaction — rather than against pre-warmed on-chain state. This
is the freshness model that perpetual-DEX execution, lending liquidations, and
oracle-priced AMM swaps require.

## Status

**v0 — prototype. Testnet only. Not for production capital.**

v0 runs a single self-operated signer and has not been independently audited.
Its purpose is to validate the architecture and ground signature-scheme and
storage decisions in measured Soroban resource cost. A multi-publisher,
audited version is future work.

## Repository layout

| Path | What it is |
|------|------------|
| `oracle_v0/` | The Soroban price-oracle contract (`cdylib`, crate `noeracle_oracle_v0`). Production entrypoint: `update_batch_ed25519_args`, which verifies signed prices inline in a consumer transaction. |
| `bench/` | Soroban contract + host-emulated tests (crate `noeracle_bench`) that measure each crypto primitive — Ed25519, secp256k1, secp256r1, BLS12-381 — in isolation, with the host budget reset between calls. The printed CPU / memory cost tables are the deliverable. |
| `scripts/` | Node end-to-end drivers. `run_oracle_bench.mjs` measures real resource and total fees per entrypoint against a live Soroban RPC; `fetch_ledger_limits.mjs` dumps live network `ConfigSettingEntry` limits. |

`bench/` and `oracle_v0/` are a matched pair: `bench/` measures crypto
primitives in the simulated host, and `scripts/run_oracle_bench.mjs` measures
the same primitives end-to-end inside real Stellar transactions. Numbers are
cross-validated between the two.

## Prerequisites

- Rust (stable) with the Soroban WASM target: `rustup target add wasm32v1-none`
- Node.js 18+ (for `scripts/`)
- Optional: the [`stellar` CLI](https://developers.stellar.org/docs/tools/cli)

## Build

The contract targets `wasm32v1-none` (Soroban's current WASM target).

```bash
cargo build -p noeracle_oracle_v0 --target wasm32v1-none --release
# or:  stellar contract build
```

## Test — cost measurement

The benchmark suite prints CPU / memory cost tables. `--nocapture` is
required: that printed output is the point, not the pass/fail result.

```bash
# all cost tables
cargo test -p noeracle_bench -- --nocapture

# a single table
cargo test -p noeracle_bench ed25519_table -- --nocapture
```

`bench/test_snapshots/` holds the Soroban test framework's deterministic
ledger snapshots — committed so cost regressions show up in diffs.

## End-to-end fee measurement

```bash
cd scripts
npm install
npm run limits:testnet    # live ConfigSettingEntry dump
npm run bench:testnet     # full oracle_v0 fee sweep
```

These require a populated `.env` at the repository root (gitignored — the
schema is documented inside the file). The testnet scripts spend testnet XLM;
the `:mainnet` variants spend real XLM.

## Signed message format

Publisher signatures cover a fixed 40-byte layout, used identically by the
contract, the benchmark suite, and the off-chain tooling:

```
asset(8 bytes) || price(i128 BE, 16) || timestamp(u64 BE, 8) || round_id(u64 BE, 8)
```

## License

MIT — see [LICENSE](LICENSE).
