# Noeracle consumer_v0 — Soroban consumer reference

A minimal Soroban contract demonstrating the inline oracle-verification pattern: the consumer's own contract calls the Noeracle oracle inside its own entrypoint, atomically verifying the signed price and using it in the same transaction.

Fork this as the starting point for a perp DEX, lending market, oracle-priced AMM, or any consumer that needs sub-second execution-time freshness.

## Reference instance (testnet)

A deployed instance is live on Stellar testnet — `demo.mjs` targets it by default:

| | |
|---|---|
| Consumer contract | [`CAECJ3WXVR4UXTFVDAQJF5L7VPR2X6WBGXDZX7UKTBAKJ4WNCPW2WD4E`](https://stellar.expert/explorer/testnet/contract/CAECJ3WXVR4UXTFVDAQJF5L7VPR2X6WBGXDZX7UKTBAKJ4WNCPW2WD4E) |
| Noeracle oracle | [`CAYIP67UDVX5UPXGN3XDAWVIEFBAVG6G7LUESEOU3NUQKTWN55W34YBG`](https://stellar.expert/explorer/testnet/contract/CAYIP67UDVX5UPXGN3XDAWVIEFBAVG6G7LUESEOU3NUQKTWN55W34YBG) |
| Network | Stellar Testnet |

You don't need to deploy your own copy to see the pattern in action — just fund a testnet account and run the demo below.

## What the contract does

`init(oracle)` — one-time setup; records the Noeracle oracle contract address.

`open_position(trader, size, assets, prices, timestamp, round_id, pubkey, sigs)` — takes the six raw `update_batch_ed25519_args` arguments alongside the position arguments. It:

1. Checks `trader.require_auth()`.
2. Calls the oracle's `update_batch_ed25519_args` via `env.invoke_contract`, forwarding the signature payload verbatim. If the signature, the staleness window, or the registered-publisher check fails, the cross-contract call aborts and this whole transaction reverts.
3. Uses the verified price (the first asset in the forwarded vectors) as the entry price and writes a `Position` to persistent storage.

`get_position(trader)` — reads a stored position.

## Run the demo

```bash
npm install
ADMIN_SECRET=S...your-testnet-secret... node demo.mjs
```

The demo fetches a fresh BTC/USD attestation from `api.noeracle.org`, builds an `open_position` call on the reference consumer, submits to testnet, and prints the resulting `Position`.

Need a testnet account? Generate one and fund it via Friendbot:

```bash
stellar keys generate --network testnet trader
stellar keys fund trader --network testnet
ADMIN_SECRET=$(stellar keys show trader) node demo.mjs
```

## Build from source

The contract targets `wasm32v1-none`.

```bash
cargo build --target wasm32v1-none --release
# WASM: target/wasm32v1-none/release/noeracle_consumer_v0_example.wasm
```

## Deploy your own instance

```bash
stellar contract deploy \
  --wasm target/wasm32v1-none/release/noeracle_consumer_v0_example.wasm \
  --source <your-identity> \
  --network testnet
# Returns: C... (your new contract id)

stellar contract invoke \
  --id <your-new-id> \
  --source <your-identity> \
  --network testnet -- \
  init --oracle CAYIP67UDVX5UPXGN3XDAWVIEFBAVG6G7LUESEOU3NUQKTWN55W34YBG
```

Then run the demo against your instance:

```bash
CONSUMER_ID=<your-new-id> ADMIN_SECRET=S... node demo.mjs
```

## The pattern in plain words

The consumer contract calls `update_batch_ed25519_args` on the oracle inside its own entrypoint, passing the oracle's six args alongside the consumer's own args. The price is verified and used atomically — if the signature is invalid or the round is stale, the entire consumer transaction reverts with no halfway state.

Full documentation: [noeracle.org/docs/integration](https://noeracle.org/docs/integration/#pattern-b--inline-verification-inside-your-contract).
