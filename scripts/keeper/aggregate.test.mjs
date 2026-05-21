// Tests for price aggregation: exchange-weighted average plus outlier and
// staleness filtering.
//
// Run: node --test scripts/keeper/   (or `npm test` from scripts/)

import test from "node:test";
import assert from "node:assert/strict";
import { aggregate, weightedAverage } from "./aggregate.mjs";

const WEIGHTS = { binance: 3, coinbase: 2, okx: 2, kraken: 1, bybit: 1 };

test("weightedAverage applies per-sample weights", () => {
  // (100*3 + 200*1) / 4 = 125
  assert.equal(
    weightedAverage([
      { price: 100, weight: 3 },
      { price: 200, weight: 1 },
    ]),
    125,
  );
});

test("aggregate weights Binance more heavily than the rest", () => {
  const now = 1_000_000;
  const samples = [
    { exchange: "binance", price: 109, ts: now },
    { exchange: "coinbase", price: 100, ts: now },
    { exchange: "okx", price: 100, ts: now },
    { exchange: "kraken", price: 100, ts: now },
    { exchange: "bybit", price: 100, ts: now },
  ];
  const out = aggregate(samples, {
    stalenessMs: 5000, outlierStddev: 3, weights: WEIGHTS, now,
  });
  // (109*3 + 100*2 + 100*2 + 100*1 + 100*1) / 9 = 927 / 9 = 103
  assert.equal(out.price, 103);
  assert.equal(out.sources, 5);
});

test("aggregate drops stale samples before averaging", () => {
  const now = 1_000_000;
  const samples = [
    { exchange: "binance", price: 100, ts: now },
    { exchange: "coinbase", price: 100, ts: now },
    { exchange: "okx", price: 100, ts: now },
    { exchange: "kraken", price: 100, ts: now },
    { exchange: "bybit", price: 9999, ts: now - 10_000 }, // 10 s stale
  ];
  const out = aggregate(samples, {
    stalenessMs: 5000, outlierStddev: 3, weights: WEIGHTS, now,
  });
  assert.equal(out.price, 100);
  assert.equal(out.sources, 4);
});

test("aggregate returns null when every sample is stale", () => {
  const now = 1_000_000;
  const samples = [{ exchange: "binance", price: 100, ts: now - 60_000 }];
  assert.equal(
    aggregate(samples, {
      stalenessMs: 5000, outlierStddev: 3, weights: WEIGHTS, now,
    }),
    null,
  );
});
