# Noeracle quickstart

Get a freshly signed price verified on Stellar testnet in a few lines.

## 1. Install

```bash
npm install @noeracle/sdk @stellar/stellar-sdk
```

## 2. Fetch a signed price and verify it on-chain

```ts
import { Noeracle } from "@noeracle/sdk";

const oracle = new Noeracle({ network: "testnet" });

// A price signed within the last ~500 ms.
const fresh = await oracle.fetchLatest(["BTC/USD"]);

// Prepend the verification op to your transaction.
tx.addOperation(fresh.toUpdateOp(ORACLE_CONTRACT_ID));
```

When that transaction lands, the contract has verified the publisher
signature and stored the price. Read it back with `get_price` — or, for
execution that must be atomic with the price, verify it inline from your own
contract. Both patterns are in the [integration guide](integration.md).

## Testnet endpoints

| | |
|---|---|
| Oracle contract | `CAYIP67UDVX5UPXGN3XDAWVIEFBAVG6G7LUESEOU3NUQKTWN55W34YBG` |
| Attestation service | `https://noeracle.fly.dev` |
| Assets | BTC/USD, ETH/USD, XLM/USD, USDC/USD |

Runnable worked examples: [`sdk/examples/`](../sdk/examples/).

## Status

v0 — testnet prototype, single signer, unaudited. **Not for production
capital.** See the [threat model](threat-model.md).
