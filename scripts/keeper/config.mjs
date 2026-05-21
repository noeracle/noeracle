// Noeracle attestation service — configuration.
//
// Each asset carries its 8-byte on-chain tag and the per-exchange ticker
// symbol used to fetch its spot price.

export const ASSETS = {
  "BTC/USD": {
    tag8: "BTCUSD",
    coinbase: "BTC-USD", binance: "BTCUSDT", kraken: "XBTUSD",
    okx: "BTC-USDT", bybit: "BTCUSDT",
  },
  "ETH/USD": {
    tag8: "ETHUSD",
    coinbase: "ETH-USD", binance: "ETHUSDT", kraken: "ETHUSD",
    okx: "ETH-USDT", bybit: "ETHUSDT",
  },
  "XLM/USD": {
    tag8: "XLMUSD",
    coinbase: "XLM-USD", binance: "XLMUSDT", kraken: "XLMUSD",
    okx: "XLM-USDT", bybit: "XLMUSDT",
  },
  "USDC/USD": {
    tag8: "USDCUSD",
    coinbase: "USDC-USD", binance: "USDCUSDT", kraken: "USDCUSD",
    okx: "USDC-USDT", bybit: "USDCUSDT",
  },
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
