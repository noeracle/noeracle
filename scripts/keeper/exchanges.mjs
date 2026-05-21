// Exchange adapters. Each adapter builds a spot-ticker URL for a symbol and
// parses the last price out of that exchange's response shape.

export const EXCHANGES = [
  {
    name: "coinbase",
    url: (s) => `https://api.exchange.coinbase.com/products/${s}/ticker`,
    parse: (j) => Number(j.price),
  },
  {
    name: "binance",
    url: (s) => `https://api.binance.com/api/v3/ticker/price?symbol=${s}`,
    parse: (j) => Number(j.price),
  },
  {
    name: "kraken",
    url: (s) => `https://api.kraken.com/0/public/Ticker?pair=${s}`,
    parse: (j) => {
      const key = Object.keys(j.result)[0];
      return Number(j.result[key].c[0]);
    },
  },
  {
    name: "okx",
    url: (s) => `https://www.okx.com/api/v5/market/ticker?instId=${s}`,
    parse: (j) => Number(j.data[0].last),
  },
  {
    name: "bybit",
    url: (s) => `https://api.bybit.com/v5/market/tickers?category=spot&symbol=${s}`,
    parse: (j) => Number(j.result.list[0].lastPrice),
  },
];

// Fetch one spot price. Returns { exchange, price, ts } or null on any
// failure (network error, timeout, bad shape, non-positive price).
export async function fetchPrice(exchange, symbol, timeoutMs = 2000) {
  const ac = new AbortController();
  const timer = setTimeout(() => ac.abort(), timeoutMs);
  try {
    const res = await fetch(exchange.url(symbol), { signal: ac.signal });
    if (!res.ok) return null;
    const price = exchange.parse(await res.json());
    if (!Number.isFinite(price) || price <= 0) return null;
    return { exchange: exchange.name, price, ts: Date.now() };
  } catch {
    return null;
  } finally {
    clearTimeout(timer);
  }
}
