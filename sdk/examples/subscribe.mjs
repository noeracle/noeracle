// Noeracle live price stream.
//
// Subscribes to the attestation service's SSE feed and prints each freshly
// signed round. A real long-running client (a keeper bot, a live dashboard)
// keeps the subscription open; this example stops after 10 rounds.
//
//   node sdk/examples/subscribe.mjs

import { Noeracle } from "../dist/index.js";

const oracle = new Noeracle({ network: "testnet" });

console.log("streaming BTC/USD and ETH/USD from the attestation service…\n");

const giveUp = setTimeout(() => {
  console.error("no rounds in 20s — is the attestation service reachable?");
  process.exit(1);
}, 20_000);

let rounds = 0;
const sub = oracle.subscribe(
  ["BTC/USD", "ETH/USD"],
  (fresh) => {
    rounds += 1;
    const cols = fresh.prices
      .map((p) => `${p.asset} = ${p.priceHuman}`)
      .join("    ");
    console.log(`round ${fresh.prices[0].roundId}    ${cols}`);
    if (rounds >= 10) {
      clearTimeout(giveUp);
      sub.close();
      console.log("\nstopped after 10 rounds (a real client keeps it open).");
      process.exit(0);
    }
  },
  (err) => console.error("stream error:", err.message),
);
