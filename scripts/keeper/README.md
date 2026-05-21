# Noeracle attestation service

Off-chain service for the Noeracle pull oracle. It polls five exchanges for
spot prices, computes a per-asset median, signs it with the publisher's
Ed25519 key, and serves the freshly signed attestation over HTTP.

## Run

```bash
cd scripts
npm install        # one-time
npm run keeper
```

Requires `NOERACLE_PUBLISHER_SECRET_HEX` (32-byte hex Ed25519 key) in the
repo-root `.env`. Optional `KEEPER_PORT` (default 8080).

## Endpoints

| Route | Description |
|-------|-------------|
| `GET /health` | Liveness, poll count, live asset count, publisher key |
| `GET /v1/latest` | Latest signed attestation for every asset |
| `GET /v1/latest/:asset` | Latest signed attestation for one asset (`BTC-USD`, `ETH-USD`, …) |
| `GET /v1/stream` | Server-Sent Events — a `prices` event every signing round |

## Attestation shape

```json
{
  "asset": "BTC/USD",
  "tag": "BTCUSD",
  "price": "654321000000",
  "price_human": 65432.1,
  "timestamp": 1748000000,
  "round_id": 3496000000,
  "sources": 5,
  "publisher": "8f8650…",
  "message": "<40-byte hex>",
  "signature": "<64-byte hex>"
}
```

`message` is the exact 40 bytes the `oracle_v0` contract verifies:
`asset(8) || price(i128 BE) || timestamp(u64 BE) || round_id(u64 BE)`. A
consumer submits the price, `timestamp`, `round_id`, `publisher`, and
`signature` to `update_batch_ed25519_args`.

## Design

- **Polling** — every 500 ms, all exchanges for all assets, in parallel.
- **Aggregation** — stale samples (>5 s) dropped; outliers beyond 3σ dropped
  when ≥3 sources are present; median of the rest.
- **Signing** — `round_id = floor(unix_ms / 500)`, deterministic and
  monotonic; the message is Ed25519-signed each round.
- Stateless and single-process — restart and it resumes signing.
