// The attestation engine: one batched price fetch per exchange each round,
// aggregated per asset into a weighted average and signed into a fresh
// attestation.

import {
  ASSETS, ROUND_WINDOW_MS, PRICE_SCALE, STALENESS_MS, OUTLIER_STDDEV, WEIGHTS,
} from "./config.mjs";
import { EXCHANGES, fetchAll } from "./exchanges.mjs";
import { aggregate } from "./aggregate.mjs";
import { buildMessage, sign, publicKeyHex } from "./message.mjs";

export function createKeeper(secretKeyHex) {
  const publisher = publicKeyHex(secretKeyHex);
  const samples = {}; // samples[asset][exchange] = { price, ts }
  const latest = {};  // latest[asset]            = attestation object
  for (const asset of Object.keys(ASSETS)) samples[asset] = {};

  // Per-exchange view of the asset table: the symbol list each venue is
  // polled with, and the symbol -> asset name reverse index.
  const perExchange = {};
  for (const [asset, def] of Object.entries(ASSETS)) {
    for (const ex of EXCHANGES) {
      const symbol = def[ex.name];
      if (!symbol) continue;
      const entry = (perExchange[ex.name] ??= { symbols: [], bySymbol: new Map() });
      entry.symbols.push(symbol);
      entry.bySymbol.set(symbol, asset);
    }
  }

  let polls = 0;

  async function pollOnce() {
    // One batched request per exchange covers every asset it lists.
    const jobs = EXCHANGES.map(async (ex) => {
      const wanted = perExchange[ex.name];
      if (!wanted) return;
      const prices = await fetchAll(ex, wanted.symbols);
      if (!prices) return;
      const ts = Date.now();
      for (const [symbol, price] of prices) {
        const asset = wanted.bySymbol.get(symbol);
        if (asset) samples[asset][ex.name] = { price, ts };
      }
    });
    await Promise.all(jobs);

    const now = Date.now();
    const roundId = Math.floor(now / ROUND_WINDOW_MS);
    const timestamp = Math.floor(now / 1000);

    for (const [asset, def] of Object.entries(ASSETS)) {
      const arr = Object.entries(samples[asset]).map(([exchange, s]) => ({
        exchange, price: s.price, ts: s.ts,
      }));
      const agg = aggregate(arr, {
        stalenessMs: STALENESS_MS, outlierStddev: OUTLIER_STDDEV,
        weights: WEIGHTS, now,
      });
      if (!agg) continue;

      const priceScaled = BigInt(Math.round(agg.price * PRICE_SCALE));
      const message = buildMessage(def.tag8, priceScaled, timestamp, roundId);
      const signature = sign(message, secretKeyHex);
      latest[asset] = {
        asset,
        tag: def.tag8,
        price: priceScaled.toString(),
        price_human: agg.price,
        timestamp,
        round_id: roundId,
        sources: agg.sources,
        publisher,
        message: message.toString("hex"),
        signature: signature.toString("hex"),
      };
    }
    polls += 1;
  }

  return {
    pollOnce,
    getLatest: (asset) => latest[asset] || null,
    getAll: () => latest,
    stats: () => {
      // Age of the freshest signed attestation, in seconds — the signal for
      // a keeper that is running but no longer signing. null before the first
      // successful round.
      const timestamps = Object.values(latest).map((a) => a.timestamp);
      const lastSignedAgeS = timestamps.length
        ? Math.floor(Date.now() / 1000) - Math.max(...timestamps)
        : null;
      return {
        polls,
        assets_live: Object.keys(latest).length,
        publisher,
        last_signed_age_s: lastSignedAgeS,
      };
    },
  };
}
