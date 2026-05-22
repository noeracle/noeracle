// The attestation engine: polls every exchange for every asset, aggregates a
// per-asset median, and signs it into a fresh attestation each round.

import {
  ASSETS, ROUND_WINDOW_MS, PRICE_SCALE, STALENESS_MS, OUTLIER_STDDEV, WEIGHTS,
} from "./config.mjs";
import { EXCHANGES, fetchPrice } from "./exchanges.mjs";
import { aggregate } from "./aggregate.mjs";
import { buildMessage, sign, publicKeyHex } from "./message.mjs";

export function createKeeper(secretKeyHex) {
  const publisher = publicKeyHex(secretKeyHex);
  const samples = {}; // samples[asset][exchange] = { price, ts }
  const latest = {};  // latest[asset]            = attestation object
  for (const asset of Object.keys(ASSETS)) samples[asset] = {};

  let polls = 0;

  async function pollOnce() {
    const jobs = [];
    for (const [asset, def] of Object.entries(ASSETS)) {
      for (const ex of EXCHANGES) {
        const symbol = def[ex.name];
        if (!symbol) continue;
        jobs.push(
          fetchPrice(ex, symbol).then((r) => {
            if (r) samples[asset][ex.name] = { price: r.price, ts: r.ts };
          }),
        );
      }
    }
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
