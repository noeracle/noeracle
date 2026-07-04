// Adapter tests: each venue's extract shape, list filtering, price guards,
// venue-level error handling, and the binance all-tickers fallback — all
// against canned fixtures via a stubbed global fetch (no network).

import test from "node:test";
import assert from "node:assert/strict";

import { EXCHANGES, fetchAll } from "./exchanges.mjs";

const byName = Object.fromEntries(EXCHANGES.map((e) => [e.name, e]));

// Run `fn` with global fetch replaced by `impl(url)`; always restores.
async function withFetch(impl, fn) {
  const orig = globalThis.fetch;
  globalThis.fetch = impl;
  try {
    return await fn();
  } finally {
    globalThis.fetch = orig;
  }
}

const json = (body, status = 200) =>
  new Response(JSON.stringify(body), { status });

test("binance: extracts asked symbols, ignores extras and missing", async () => {
  const fixture = [
    { symbol: "BTCUSDT", price: "63286.56" },
    { symbol: "ETHUSDT", price: "1803.48" },
    { symbol: "JUNKUSDT", price: "1" },
  ];
  const out = await withFetch(
    async () => json(fixture),
    () => fetchAll(byName.binance, ["BTCUSDT", "ETHUSDT", "GONEUSDT"]),
  );
  assert.deepEqual(
    [...out.entries()].sort(),
    [["BTCUSDT", 63286.56], ["ETHUSDT", 1803.48]],
  );
});

test("binance: HTTP 400 switches to all-tickers for later rounds", async () => {
  const ex = byName.binance;
  const urls = [];
  try {
    const first = await withFetch(
      async (url) => {
        urls.push(url);
        return json({ code: -1121, msg: "Invalid symbol." }, 400);
      },
      () => fetchAll(ex, ["BTCUSDT", "GONEUSDT"]),
    );
    assert.equal(first, null);
    assert.equal(ex.allMode, true);

    const second = await withFetch(
      async (url) => {
        urls.push(url);
        return json([{ symbol: "BTCUSDT", price: "63000" }]);
      },
      () => fetchAll(ex, ["BTCUSDT", "GONEUSDT"]),
    );
    assert.ok(urls[0].includes("symbols="));
    assert.ok(!urls[1].includes("symbols="), "fallback must hit all-tickers");
    assert.deepEqual([...second.entries()], [["BTCUSDT", 63000]]);
  } finally {
    delete ex.allMode;
  }
});

test("coinbase: keyed by product_id", async () => {
  const fixture = {
    products: [
      { product_id: "BTC-USDT", price: "63235.43" },
      { product_id: "LTC-USD", price: "45.5" },
    ],
  };
  const out = await withFetch(
    async () => json(fixture),
    () => fetchAll(byName.coinbase, ["BTC-USDT", "LTC-USD", "NOPE-USD"]),
  );
  assert.deepEqual(
    [...out.entries()].sort(),
    [["BTC-USDT", 63235.43], ["LTC-USD", 45.5]],
  );
});

test("kraken: keyed by canonical pair name, last-trade price", async () => {
  const fixture = {
    error: [],
    result: {
      XBTUSDT: { c: ["63259.0", "0.01"] },
      XXLMZUSD: { c: ["0.213628", "120.5"] },
    },
  };
  const out = await withFetch(
    async () => json(fixture),
    () => fetchAll(byName.kraken, ["XBTUSDT", "XXLMZUSD"]),
  );
  assert.deepEqual(
    [...out.entries()].sort(),
    [["XBTUSDT", 63259], ["XXLMZUSD", 0.213628]],
  );
});

test("kraken: venue-level error yields null", async () => {
  const out = await withFetch(
    async () => json({ error: ["EQuery:Unknown asset pair"] }),
    () => fetchAll(byName.kraken, ["XBTUSDT", "GONEUSD"]),
  );
  assert.equal(out, null);
});

test("okx: filters the full spot list down to asked instIds", async () => {
  const fixture = {
    code: "0",
    data: [
      { instId: "BTC-USDT", last: "63283.2" },
      { instId: "NOPE-USDT", last: "1" },
    ],
  };
  const out = await withFetch(
    async () => json(fixture),
    () => fetchAll(byName.okx, ["BTC-USDT"]),
  );
  assert.deepEqual([...out.entries()], [["BTC-USDT", 63283.2]]);
});

test("bybit: filters the full spot list down to asked symbols", async () => {
  const fixture = {
    result: {
      list: [
        { symbol: "BTCUSDT", lastPrice: "63283.6" },
        { symbol: "NOPEUSDT", lastPrice: "1" },
      ],
    },
  };
  const out = await withFetch(
    async () => json(fixture),
    () => fetchAll(byName.bybit, ["BTCUSDT"]),
  );
  assert.deepEqual([...out.entries()], [["BTCUSDT", 63283.6]]);
});

test("non-positive and non-numeric prices are dropped", async () => {
  const fixture = [
    { symbol: "AUSDT", price: "0" },
    { symbol: "BUSDT", price: "-3" },
    { symbol: "CUSDT", price: "nope" },
    { symbol: "DUSDT", price: "1.5" },
  ];
  const out = await withFetch(
    async () => json(fixture),
    () => fetchAll(byName.binance, ["AUSDT", "BUSDT", "CUSDT", "DUSDT"]),
  );
  assert.deepEqual([...out.entries()], [["DUSDT", 1.5]]);
});

test("network failure yields null, never throws", async () => {
  const out = await withFetch(
    async () => {
      throw new TypeError("fetch failed");
    },
    () => fetchAll(byName.okx, ["BTC-USDT"]),
  );
  assert.equal(out, null);
});
