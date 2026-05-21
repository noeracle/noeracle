// Noeracle attestation service entry point. Polls exchanges on a fixed
// cadence and serves freshly signed price attestations over HTTP.

import dotenv from "dotenv";
dotenv.config({ path: new URL("../../.env", import.meta.url) });

import { createKeeper } from "./keeper.mjs";
import { createServer } from "./server.mjs";
import { PORT, POLL_INTERVAL_MS } from "./config.mjs";

const secretKeyHex = process.env.NOERACLE_PUBLISHER_SECRET_HEX;
if (!secretKeyHex || secretKeyHex.length !== 64) {
  console.error("NOERACLE_PUBLISHER_SECRET_HEX (32-byte hex) missing from .env");
  process.exit(1);
}

const keeper = createKeeper(secretKeyHex);

// Catch a publisher key that does not match the one registered on-chain.
const expectedPublic = process.env.NOERACLE_PUBLISHER_PUBLIC_HEX;
if (expectedPublic && keeper.stats().publisher !== expectedPublic) {
  console.error(
    `publisher key mismatch: derived ${keeper.stats().publisher}, ` +
      `.env NOERACLE_PUBLISHER_PUBLIC_HEX is ${expectedPublic}`,
  );
  process.exit(1);
}

const startedAt = Date.now();

async function loop() {
  const t0 = Date.now();
  try {
    await keeper.pollOnce();
  } catch (err) {
    console.error("poll error:", err.message);
  }
  setTimeout(loop, Math.max(0, POLL_INTERVAL_MS - (Date.now() - t0)));
}

createServer(keeper, startedAt).listen(PORT, () => {
  console.log(
    `Noeracle attestation service listening on :${PORT} ` +
      `(publisher ${keeper.stats().publisher.slice(0, 16)}…)`,
  );
});
loop();
