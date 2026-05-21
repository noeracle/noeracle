// Noeracle cached-price read.
//
// Reads the last price stored on-chain for an asset via the contract's
// get_price — a free, best-effort cache (no transaction, no fee). Use this
// for non-execution reads; when freshness must be guaranteed, bring a signed
// update inline instead (see e2e.mjs).
//
//   node sdk/examples/read-price.mjs

import { readFileSync } from "node:fs";
import {
  Contract,
  Keypair,
  Networks,
  rpc,
  scValToNative,
  TransactionBuilder,
  xdr,
} from "@stellar/stellar-sdk";

const env = Object.fromEntries(
  readFileSync(new URL("../../.env", import.meta.url), "utf8")
    .split("\n")
    .filter((l) => l && !l.startsWith("#") && l.includes("="))
    .map((l) => {
      const i = l.indexOf("=");
      return [l.slice(0, i).trim(), l.slice(i + 1).trim().replace(/^"|"$/g, "")];
    }),
);
const server = new rpc.Server(
  env.TESTNET_RPC || "https://soroban-testnet.stellar.org",
);
const source = Keypair.fromSecret(env.NOERACLE_ADMIN_SECRET);

// get_price is a read — simulate it, never submit. The source account only
// forms the envelope; nothing is sent on-chain and no fee is paid.
const tx = new TransactionBuilder(await server.getAccount(source.publicKey()), {
  fee: "1000000",
  networkPassphrase: Networks.TESTNET,
})
  .addOperation(
    new Contract(env.TESTNET_ORACLE_V0).call(
      "get_price",
      xdr.ScVal.scvBytes(Buffer.from("BTCUSD\0\0", "ascii")),
    ),
  )
  .setTimeout(60)
  .build();

const sim = await server.simulateTransaction(tx);
if (sim.error) throw new Error(`simulation failed: ${sim.error}`);
const entry = scValToNative(sim.result.retval);

if (!entry) {
  console.log("get_price(BTC/USD): no cached price (TTL expired, or none written yet)");
} else {
  const age = Math.floor(Date.now() / 1000) - Number(entry.timestamp);
  console.log(
    `get_price(BTC/USD): price=${entry.price}  round=${entry.round_id}  (signed ${age}s ago)`,
  );
}
