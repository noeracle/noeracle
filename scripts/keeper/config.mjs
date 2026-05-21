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
export const POLL_INTERVAL_MS = 500;
export const ROUND_WINDOW_MS = 500;

// i128 price precision: scale floats by 1e7 (Stellar's standard precision).
export const PRICE_SCALE = 10_000_000;

// Drop exchange samples not refreshed within this window.
export const STALENESS_MS = 5_000;

// Drop samples beyond this many standard deviations from the mean
// (only applied when at least 3 sources are present).
export const OUTLIER_STDDEV = 3;

export const PORT = Number(process.env.KEEPER_PORT || 8080);
