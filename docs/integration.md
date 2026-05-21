# Integrating Noeracle

Noeracle is a pull oracle: you fetch a freshly signed price off-chain and
submit it yourself, so the price your transaction acts on is fresh at
execution time rather than as of the last keeper write. There are two ways
to integrate.

## Mode 1 — off-chain (application layer)

Your backend, bot, or frontend fetches a price with the SDK and submits the
verification operation as its own transaction. Good for warming the on-chain
cache, dashboards, and keepers.

```ts
import { Noeracle } from "@noeracle/sdk";

const oracle = new Noeracle({ network: "testnet" });
const fresh = await oracle.fetchLatest(["BTC/USD", "ETH/USD"]);

tx.addOperation(fresh.toUpdateOp(ORACLE_CONTRACT_ID));
await server.sendTransaction(tx);
```

Once the operation lands, any contract can read the price for free with
`get_price` (a best-effort cache — see [Storage & freshness](#storage--freshness)).

For a live feed, subscribe instead of polling:

```ts
const sub = oracle.subscribe(["BTC/USD"], (fresh) => {
  console.log(fresh.price("BTC/USD").priceHuman);
});
```

## Mode 2 — on-chain (inside your contract)

For execution that must be **atomic** with the price — perp fills, lending
liquidations, oracle-priced swaps — your Soroban contract verifies the price
itself by cross-calling the oracle within its own invocation. A Stellar
transaction carries a single Soroban operation, so this is how the price
check and your logic stay in one atomic call.

**Your contract** imports the oracle and cross-calls it:

```rust
#![no_std]
use soroban_sdk::{contract, contractimpl, Address, BytesN, Env, Vec};

mod oracle {
    soroban_sdk::contractimport!(file = "noeracle_oracle_v0.wasm");
}

#[contract]
pub struct MyDex;

#[contractimpl]
impl MyDex {
    pub fn open_position(
        env: Env,
        trader: Address,
        oracle_id: Address,
        market: BytesN<8>,
        // the signed price, forwarded from @noeracle/sdk:
        assets: Vec<BytesN<8>>,
        prices: Vec<i128>,
        timestamp: u64,
        round_id: u64,
        pubkey: BytesN<32>,
        sigs: Vec<BytesN<64>>,
    ) {
        trader.require_auth();
        let oracle = oracle::Client::new(&env, &oracle_id);

        // 1. Verify the freshly signed price on-chain.
        oracle.update_batch_ed25519_args(
            &assets, &prices, &timestamp, &round_id, &pubkey, &sigs,
        );

        // 2. Read the just-verified price and run your logic against it.
        let price = oracle.get_price(&market).unwrap().price;
        // ... open the position at `price` ...
    }
}
```

**Your client** forwards the signed price into that call. `fresh.updateArgs()`
returns the six oracle arguments as ScVals, ready to splice in:

```ts
import { Contract } from "@stellar/stellar-sdk";

const fresh = await oracle.fetchLatest(["BTC/USD"]);

const op = new Contract(MY_DEX_ID).call(
  "open_position",
  /* your own args: trader, oracle_id, market, … */
  ...fresh.updateArgs(), // assets, prices, timestamp, round_id, pubkey, sigs
);
tx.addOperation(op);
```

## Contract API

Functions a consumer calls on `oracle_v0`:

| Function | Purpose |
|----------|---------|
| `update_batch_ed25519_args(assets, prices, timestamp, round_id, pubkey, sigs)` | Verify signed price(s) and store them. Returns `Result<(), Error>`. |
| `get_price(asset) -> Option<PriceEntry>` | Read the stored price for one asset. |

`PriceEntry { price: i128, timestamp: u64, round_id: u64 }` — prices are
integers scaled by 1e7.

`Error`: `NotInitialized`, `BatchLengthMismatch`, `UnknownPublisher`,
`StalePrice` (signed more than 60 s ago). A round older than the stored one
is a silent no-op, not an error — a consumer's transaction never fails
because another consumer used a newer price first.

## Signed message format

The publisher signs a fixed 40-byte message; the contract rebuilds and
verifies it:

```
asset(8) || price(i128 BE, 16) || timestamp(u64 BE, 8) || round_id(u64 BE, 8)
```

The SDK and the contract handle this — you never build it by hand.

## Storage & freshness

- **Inline verification** (`update_batch_ed25519_args`) is the only
  freshness-*guaranteed* path: the price is whatever the publisher signed
  ~500 ms before you fetched it.
- **`get_price`** is a free, best-effort cache — it returns whatever the last
  pull-mode transaction wrote, or `None` once the entry's TTL expires. Use it
  for non-execution reads; do not trigger liquidations or fills off it.

## Failure modes

| Situation | What happens / what to do |
|-----------|---------------------------|
| Attestation service unreachable | `fetchLatest` throws `AttestationServiceError` — retry with backoff. |
| Price older than 60 s on submit | `update_batch_ed25519_args` fails `StalePrice` — re-fetch and resubmit. |
| Asset not served | `fetchLatest` throws `AssetUnavailableError`. |
| Transaction didn't land in time | Re-fetch a fresher price and resubmit. |
