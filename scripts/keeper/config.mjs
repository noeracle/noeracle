// Noeracle attestation service — configuration.
//
// Each asset carries its 8-byte on-chain tag and the per-exchange ticker
// symbol used to fetch its spot price. Pairs are USDT-quoted wherever the
// venue lists one, falling back to the venue's USD book otherwise (the
// aggregate treats USDT parity as USD — spreads between the two stay well
// inside the cross-venue spread).
//
// Venue symbol conventions:
//   - kraken:   CANONICAL pair names, exactly as returned by the batch
//               Ticker endpoint (legacy pairs differ from their altname,
//               e.g. XLMUSD -> XXLMZUSD, ZECUSD -> XZECZUSD; look new ones
//               up in /0/public/AssetPairs: result key = canonical,
//               `altname` = the human name).
//   - coinbase: Advanced Trade product ids (BASE-QUOTE).
//   - binance / bybit: concatenated spot symbols; okx: BASE-QUOTE instId.
//
// An asset omits a venue that does not list it (e.g. TRX on Coinbase); the
// aggregator simply works with the venues that remain. Keep every asset on
// at least 3 venues so outlier rejection stays meaningful.

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
    coinbase: "XLM-USDT", binance: "XLMUSDT", kraken: "XXLMZUSD",
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
  "BNB/USD": {
    tag8: "BNBUSD",
    coinbase: "BNB-USD", binance: "BNBUSDT", kraken: "BNBUSDT",
    okx: "BNB-USDT", bybit: "BNBUSDT",
  },
  "TRX/USD": {
    tag8: "TRXUSD",
    binance: "TRXUSDT", kraken: "TRXUSD",
    okx: "TRX-USDT", bybit: "TRXUSDT",
  },
  "HYPE/USD": {
    tag8: "HYPEUSD",
    coinbase: "HYPE-USD", kraken: "HYPEUSD",
    okx: "HYPE-USDT", bybit: "HYPEUSDT",
  },
  "DOGE/USD": {
    tag8: "DOGEUSD",
    coinbase: "DOGE-USDT", binance: "DOGEUSDT", kraken: "XDGUSDT",
    okx: "DOGE-USDT", bybit: "DOGEUSDT",
  },
  "ZEC/USD": {
    tag8: "ZECUSD",
    coinbase: "ZEC-USD", binance: "ZECUSDT", kraken: "XZECZUSD",
    okx: "ZEC-USDT",
  },
  "LINK/USD": {
    tag8: "LINKUSD",
    coinbase: "LINK-USDT", binance: "LINKUSDT", kraken: "LINKUSDT",
    okx: "LINK-USDT", bybit: "LINKUSDT",
  },
  "BCH/USD": {
    tag8: "BCHUSD",
    coinbase: "BCH-USD", binance: "BCHUSDT", kraken: "BCHUSDT",
    okx: "BCH-USDT", bybit: "BCHUSDT",
  },
  "LTC/USD": {
    tag8: "LTCUSD",
    coinbase: "LTC-USD", binance: "LTCUSDT", kraken: "LTCUSDT",
    okx: "LTC-USDT", bybit: "LTCUSDT",
  },
  // Tokenized gold. These price the TOKEN (Paxos / Tether gold), not LBMA
  // spot — the tags stay PAXG/XAUT on purpose so the basis is never hidden.
  "PAXG/USD": {
    tag8: "PAXGUSD",
    coinbase: "PAXG-USD", binance: "PAXGUSDT", kraken: "PAXGUSD",
    okx: "PAXG-USDT",
  },
  "XAUT/USD": {
    tag8: "XAUTUSD",
    binance: "XAUTUSDT", kraken: "XAUTUSD",
    okx: "XAUT-USDT", bybit: "XAUTUSDT",
  },
  "PUMP/USD": {
    tag8: "PUMPUSD",
    coinbase: "PUMP-USD", binance: "PUMPUSDT", kraken: "PUMPUSD",
    okx: "PUMP-USDT", bybit: "PUMPUSDT",
  },
  "UNI/USD": {
    tag8: "UNIUSD",
    coinbase: "UNI-USD", binance: "UNIUSDT", kraken: "UNIUSD",
    okx: "UNI-USDT", bybit: "UNIUSDT",
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

// Freshness SLA. Past this, /health reports `fresh: false` and status
// "degraded" — but still HTTP 200, so the machine stays in the Fly load
// balancer and keeps serving the last signed price. 10 s ≈ 20 missed rounds:
// wide enough not to trip on a brief exchange hiccup.
export const HEALTH_STALENESS_S = 10;

// Liveness bound. Past this, /health returns 503 (Fly drops the machine from
// rotation) AND the keeper's watchdog exits the process so Fly restarts the
// machine — a fresh machine gets a new egress IP, which clears the exchange
// rate-limit ban that is the usual cause of a sustained signing stall. 60 s
// matches the on-chain staleness bound: the contract rejects any price older
// than this, so a staler attestation is useless to serve anyway. Decoupling
// these two bounds is deliberate — a brief stall must not black out the
// service (return nothing); only a sustained one warrants pulling the machine.
export const SERVE_MAX_STALENESS_S = 60;

export const PORT = Number(process.env.KEEPER_PORT || 8080);
