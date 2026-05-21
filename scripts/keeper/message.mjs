// The signed price message — the single source of truth for the 40-byte
// layout the oracle_v0 contract verifies. Must stay byte-identical to
// oracle_v0::build_msg (Rust) and the bench / SDK encoders:
//
//   asset(8) || price(i128 BE, 16) || timestamp(u64 BE, 8) || round_id(u64 BE, 8)

import { ed25519 } from "@noble/curves/ed25519";

// 8-byte asset tag: ASCII bytes of the tag, null-padded (or truncated) to 8.
export function assetTag8(tag) {
  const buf = Buffer.alloc(8);
  Buffer.from(tag, "ascii").copy(buf, 0, 0, 8);
  return buf;
}

// Build the 40-byte message. priceScaled is a BigInt (price * 1e7).
export function buildMessage(tag, priceScaled, timestamp, roundId) {
  const buf = Buffer.alloc(40);
  assetTag8(tag).copy(buf, 0);
  // price: i128, big-endian, occupying bytes 8..23.
  let v = BigInt(priceScaled);
  for (let i = 23; i >= 8; i--) {
    buf[i] = Number(v & 0xffn);
    v >>= 8n;
  }
  buf.writeBigUInt64BE(BigInt(timestamp), 24);
  buf.writeBigUInt64BE(BigInt(roundId), 32);
  return buf;
}

// Ed25519-sign a message with a 32-byte secret key (hex). Returns 64 bytes.
export function sign(message, secretKeyHex) {
  return Buffer.from(ed25519.sign(message, Buffer.from(secretKeyHex, "hex")));
}

// Derive the 32-byte public key (hex) from a 32-byte secret key (hex).
export function publicKeyHex(secretKeyHex) {
  return Buffer.from(
    ed25519.getPublicKey(Buffer.from(secretKeyHex, "hex")),
  ).toString("hex");
}
