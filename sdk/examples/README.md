# Noeracle SDK examples

Worked examples for integrating the Noeracle pull oracle. From the repo root,
build the SDK once (`npm install --prefix sdk && npm --prefix sdk run build`),
then run any example with `node`.

| Example | Pattern |
|---------|---------|
| `e2e.mjs` | **The core pull path** — fetch a freshly signed price, bundle the `update_batch_ed25519_args` verification op into a transaction, submit it on testnet, and confirm the verified price on-chain. |
| `subscribe.mjs` | **Live prices** — subscribe to the attestation service's SSE stream and receive each freshly signed round. For long-running clients (bots, dashboards). |
| `read-price.mjs` | **Free cached read** — read the last on-chain price via `get_price` with no transaction. Best-effort; use the `e2e.mjs` inline path when freshness must be guaranteed. |

```bash
node sdk/examples/e2e.mjs          # or: npm --prefix sdk run demo
node sdk/examples/subscribe.mjs
node sdk/examples/read-price.mjs
```

`e2e.mjs` and `read-price.mjs` read the testnet contract id and a funded
source key from the repo-root `.env`.

## Integrating from a Soroban contract

These examples cover off-chain (application-layer) integration. A Soroban
*consumer contract* verifies a price atomically by cross-calling the oracle's
`update_batch_ed25519_args` within its own invocation, then reading
`get_price` — all in one operation. See the integration guide for the full
on-chain pattern.
