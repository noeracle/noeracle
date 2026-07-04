// Exchange adapters — one batched spot-ticker request per exchange per round.
//
// Per-symbol polling multiplies request rate by asset count (15 assets × 5
// venues × 2 rounds/s = 150 req/s) and is what got the egress IPs rate-limit
// banned on 2026-06-02. Batching pins the total at one request per venue per
// round — 10 req/s per machine — regardless of how many assets we serve.
//
// Venue batch surfaces differ:
//   - binance:  ticker/price?symbols=[…]   exact list; HTTP 400 if any symbol
//               in the list is unknown (e.g. delisted) → adapter falls back to
//               the all-tickers endpoint for the rest of the process lifetime.
//   - coinbase: Advanced Trade market/products?product_ids=…  exact list,
//               public (no auth), lenient — unknown ids are silently dropped.
//   - kraken:   Ticker?pair=a,b,c  exact list of CANONICAL pair names (the
//               response is keyed by them); errors the whole request on an
//               unknown pair, which stays visible in the logs until config
//               is fixed.
//   - okx / bybit: no multi-symbol filter — fetch the full spot ticker list
//               (~400 KB / ~180 KB) and filter locally.

export const EXCHANGES = [
  {
    name: "coinbase",
    url: (symbols) =>
      "https://api.coinbase.com/api/v3/brokerage/market/products?" +
      symbols.map((s) => `product_ids=${s}`).join("&"),
    extract: (j) =>
      new Map((j.products ?? []).map((p) => [p.product_id, Number(p.price)])),
  },
  {
    name: "binance",
    url: (symbols) =>
      symbols
        ? "https://api.binance.com/api/v3/ticker/price?symbols=" +
          encodeURIComponent(JSON.stringify(symbols))
        : "https://api.binance.com/api/v3/ticker/price",
    extract: (j) =>
      new Map((Array.isArray(j) ? j : []).map((t) => [t.symbol, Number(t.price)])),
    // A delisted symbol 400s the whole list — switch to all-tickers and keep
    // serving every symbol that still exists (recovers list mode on restart).
    fallbackToAllOn400: true,
  },
  {
    name: "kraken",
    url: (symbols) =>
      `https://api.kraken.com/0/public/Ticker?pair=${symbols.join(",")}`,
    extract: (j) =>
      new Map(
        Object.entries(j.result ?? {}).map(([pair, t]) => [pair, Number(t.c[0])]),
      ),
    error: (j) => (j.error?.length ? j.error.join("; ") : null),
  },
  {
    name: "okx",
    url: () => "https://www.okx.com/api/v5/market/tickers?instType=SPOT",
    extract: (j) =>
      new Map((j.data ?? []).map((t) => [t.instId, Number(t.last)])),
  },
  {
    name: "bybit",
    url: () => "https://api.bybit.com/v5/market/tickers?category=spot",
    extract: (j) =>
      new Map((j.result?.list ?? []).map((t) => [t.symbol, Number(t.lastPrice)])),
  },
];

// Fetch every requested symbol's spot price from one exchange in a single
// request. Resolves to Map(symbol -> price) holding only the symbols the
// venue returned with a finite positive price, or null when the whole
// request failed (network error, timeout, HTTP error, venue-level error) —
// the poll loop then just skips this venue for the round.
export async function fetchAll(exchange, symbols, timeoutMs = 2000) {
  const ac = new AbortController();
  const timer = setTimeout(() => ac.abort(), timeoutMs);
  try {
    const url = exchange.url(exchange.allMode ? null : symbols);
    const res = await fetch(url, { signal: ac.signal });
    if (!res.ok) {
      if (res.status === 400 && exchange.fallbackToAllOn400 && !exchange.allMode) {
        exchange.allMode = true;
        console.error(
          `[${exchange.name}] symbol list rejected (HTTP 400) — a symbol was ` +
            "likely delisted; falling back to all-tickers, check config",
        );
      }
      return null;
    }
    const json = await res.json();
    const errMsg = exchange.error?.(json);
    if (errMsg) {
      console.error(`[${exchange.name}] venue error: ${errMsg}`);
      return null;
    }
    const all = exchange.extract(json);
    const out = new Map();
    for (const s of symbols) {
      const price = all.get(s);
      if (Number.isFinite(price) && price > 0) out.set(s, price);
    }
    return out;
  } catch {
    return null;
  } finally {
    clearTimeout(timer);
  }
}
