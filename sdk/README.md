# @noeracle/sdk

TypeScript SDK for [Noeracle](https://github.com/noeracle/noeracle) — the
pull-based price oracle on Stellar.

Fetch a freshly signed price off-chain and bundle its on-chain verification
into your own transaction, so your contract executes against a price signed
within the last ~500 ms rather than pre-warmed state.

## Install

```bash
npm install @noeracle/sdk @stellar/stellar-sdk
```

## Use

```ts
import { Noeracle } from "@noeracle/sdk";

const oracle = new Noeracle({ network: "testnet" });

// Fetch fresh signed prices for the assets your transaction needs.
const fresh = await oracle.fetchLatest(["BTC/USD"]);

// Prepend the verification op, then your own application op(s).
tx.addOperation(fresh.toUpdateOp(ORACLE_CONTRACT_ID));
tx.addOperation(myContract.call("open_position" /* , ... */));
```

`toUpdateOp` builds the `update_batch_ed25519_args` operation, which verifies
the publisher signature on chain and stores the price. Your application op
then reads the just-verified price via the contract's `get_price`.

## API

| | |
|---|---|
| `new Noeracle({ network?, attestationUrl?, contractId? })` | Create a client |
| `oracle.fetchLatest(assets: string[]): Promise<Fresh>` | Fetch signed prices |
| `oracle.subscribe(assets, onUpdate, onError?): Subscription` | Live SSE price stream |
| `fresh.toUpdateOp(contractId?): xdr.Operation` | Build the verification op |
| `fresh.prices: PriceEntry[]` | Decoded prices |
| `fresh.price(asset): PriceEntry` | One decoded price |

Errors: `NoeracleError`, `AttestationServiceError`, `AssetUnavailableError`,
`StalePriceError`, `InconsistentRoundError`.

## Status

v0 — testnet prototype. Not for production capital.
