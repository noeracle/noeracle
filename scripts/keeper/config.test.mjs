// Config invariants: the asset table must stay compatible with the 8-byte
// on-chain tag, the adapter set, and the aggregation guarantees.

import test from "node:test";
import assert from "node:assert/strict";

import { ASSETS, WEIGHTS } from "./config.mjs";
import { EXCHANGES } from "./exchanges.mjs";

const exchangeNames = new Set(EXCHANGES.map((e) => e.name));

test("asset names are BASE/USD", () => {
  for (const asset of Object.keys(ASSETS)) {
    assert.match(asset, /^[A-Z0-9]{2,7}\/USD$/, asset);
  }
});

test("tags are ASCII, at most 8 bytes, unique", () => {
  const seen = new Set();
  for (const [asset, def] of Object.entries(ASSETS)) {
    assert.match(def.tag8, /^[A-Z0-9]{1,8}$/, `${asset} tag8`);
    assert.ok(Buffer.byteLength(def.tag8, "ascii") <= 8, `${asset} tag8 length`);
    assert.ok(!seen.has(def.tag8), `${asset} tag8 duplicates another asset`);
    seen.add(def.tag8);
  }
});

test("every asset lists at least 3 known venues", () => {
  for (const [asset, def] of Object.entries(ASSETS)) {
    const venues = Object.keys(def).filter((k) => k !== "tag8");
    assert.ok(venues.length >= 3, `${asset} has only ${venues.length} venues`);
    for (const v of venues) {
      assert.ok(exchangeNames.has(v), `${asset} references unknown venue ${v}`);
    }
  }
});

test("per-venue symbols are unique within each venue", () => {
  for (const name of exchangeNames) {
    const symbols = Object.values(ASSETS)
      .map((def) => def[name])
      .filter(Boolean);
    assert.equal(new Set(symbols).size, symbols.length, name);
  }
});

test("every venue in WEIGHTS is a known exchange, and vice versa", () => {
  assert.deepEqual(
    Object.keys(WEIGHTS).sort(),
    [...exchangeNames].sort(),
  );
});
