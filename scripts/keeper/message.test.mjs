// Conformance: pins the keeper's 40-byte message encoder + signer to a fixed
// golden vector. The same vector is verified on-chain by the Rust test
// `conformance_with_js_keeper_encoder` in oracle_v0/src/test.rs — if the
// 40-byte format drifts in either language, one of the two tests fails.
//
// Run: node --test scripts/keeper/   (or `npm test` from scripts/)

import test from "node:test";
import assert from "node:assert/strict";
import { assetTag8, buildMessage, sign, publicKeyHex } from "./message.mjs";

const GOLDEN = {
  secret: "0101010101010101010101010101010101010101010101010101010101010101",
  pubkey: "8a88e3dd7409f195fd52db2d3cba5d72ca6709bf1d94121bf3748801b40f6f5c",
  asset: "BTCUSD",
  price: 6543210000000n,
  timestamp: 1700000000,
  roundId: 1,
  message:
    "42544355534400000000000000000000000005f375b52e80000000006553f1000000000000000001",
  signature:
    "effc695c3dd70bc5da1bcea2475739340951a1fce74a6fd9e3c8bebae3147e7334b9d7f0bdb4d0770c94c8d7ad80094ddef708c30eddcb7a5b5e7a0f081e8200",
};

test("asset tag pads/truncates to 8 bytes", () => {
  assert.equal(assetTag8("BTCUSD").toString("hex"), "4254435553440000");
  assert.equal(assetTag8("BTCUSD").length, 8);
});

test("buildMessage matches the golden 40-byte vector", () => {
  const msg = buildMessage(
    GOLDEN.asset, GOLDEN.price, GOLDEN.timestamp, GOLDEN.roundId,
  );
  assert.equal(msg.length, 40);
  assert.equal(msg.toString("hex"), GOLDEN.message);
});

test("sign reproduces the golden signature", () => {
  const msg = Buffer.from(GOLDEN.message, "hex");
  assert.equal(sign(msg, GOLDEN.secret).toString("hex"), GOLDEN.signature);
});

test("publicKeyHex derives the golden public key", () => {
  assert.equal(publicKeyHex(GOLDEN.secret), GOLDEN.pubkey);
});
