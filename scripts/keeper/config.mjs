// Noeracle attestation service — configuration.
//
// Each asset carries its 8-byte on-chain tag and the per-exchange ticker
// symbol used to fetch its spot price. Pairs are USDT-quoted wherever the
// venue lists one; Kraken XLM and Coinbase USDC fall back to USD, as neither
// venue lists a USDT pair for that asset.

export const ASSETS = {
  "BTC/USD": {
    tag8: "BTCUSD",
    coinbase: "BTC-USDT", binance: "BTCUSDT", kraken: "XBTUSDT",
    okx: "BTC-USDT", bybit: "BTCUSDT",
  },
  "ETH/USD": {
    tag8: "ETHUSD",
    coinbase: "ETH-USDT", binance: "ETHUSDT", kraken: "ETHUSDT",
    okx: "ETH-USDT", bybit: "ETHUSDT",
  },
  "XLM/USD": {
    tag8: "XLMUSD",
    coinbase: "XLM-USDT", binance: "XLMUSDT", kraken: "XLMUSD",
    okx: "XLM-USDT", bybit: "XLMUSDT",
  },
  "USDC/USD": {
    tag8: "USDCUSD",
    coinbase: "USDC-USD", binance: "USDCUSDT", kraken: "USDCUSDT",
    okx: "USDC-USDT", bybit: "USDCUSDT",
  },
  "SOL/USD": {
    tag8: "SOLUSD",
    coinbase: "SOL-USDT", binance: "SOLUSDT", kraken: "SOLUSDT",
    okx: "SOL-USDT", bybit: "SOLUSDT",
  },
  "XRP/USD": {
    tag8: "XRPUSD",
    coinbase: "XRP-USDT", binance: "XRPUSDT", kraken: "XRPUSDT",
    okx: "XRP-USDT", bybit: "XRPUSDT",
  },
  "ADA/USD": {
    tag8: "ADAUSD",
    coinbase: "ADA-USDT", binance: "ADAUSDT", kraken: "ADAUSDT",
    okx: "ADA-USDT", bybit: "ADAUSDT",
  },
};

// Per-exchange weight in the aggregated price (see aggregate.mjs).
export const WEIGHTS = {
  binance: 3,
  coinbase: 2,
  okx: 2,
  kraken: 1,
  bybit: 1,
};

// Poll cadence and the round-id window (both 500 ms — see ADR 007:
// round_id = floor(unix_ms / ROUND_WINDOW_MS), deterministic and monotonic).
// Matches the published v0 testnet SLA: a fetched attestation is signed
// within the last ~500 ms.
export const POLL_INTERVAL_MS = 500;
export const ROUND_WINDOW_MS = 500;

// i128 price precision: scale floats by 1e7 (Stellar's standard precision).
export const PRICE_SCALE = 10_000_000;

// Drop exchange samples not refreshed within this window.
export const STALENESS_MS = 5_000;

// Drop samples beyond this many standard deviations from the mean
// (only applied when at least 3 sources are present).
export const OUTLIER_STDDEV = 3;

// /health reports "degraded" (HTTP 503) once the freshest signed attestation
// is older than this — the keeper is up but no longer signing, so Fly restarts
// the machine and uptime monitors fire. 10 s ≈ 20 missed rounds: wide enough
// not to trip on a brief exchange hiccup.
export const HEALTH_STALENESS_S = 10;

export const PORT = Number(process.env.KEEPER_PORT || 8080);
