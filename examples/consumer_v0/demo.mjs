// Open a position via the reference consumer, with a freshly signed
// Noeracle price verified inline. Run from this directory:
//
//   npm install
//   ADMIN_SECRET=S... node demo.mjs
//
// Optional overrides:
//   CONSUMER_ID=C...     # your own deployed consumer instance
//   ORACLE=C...          # a different oracle contract
//   RPC_URL=...          # a different Stellar RPC endpoint

import { Noeracle } from "@noeracle/sdk";
import {
  Contract,
  Keypair,
  Networks,
  TransactionBuilder,
  nativeToScVal,
  rpc,
  scValToNative,
} from "@stellar/stellar-sdk";

// The reference consumer instance on Stellar testnet — replace via CONSUMER_ID
// if you've deployed your own. See README.md.
const REFERENCE_CONSUMER = "CAECJ3WXVR4UXTFVDAQJF5L7VPR2X6WBGXDZX7UKTBAKJ4WNCPW2WD4E";

const RPC_URL = process.env.RPC_URL || "https://soroban-testnet.stellar.org";
const NETWORK = Networks.TESTNET;
const ORACLE =
  process.env.ORACLE ||
  "CAYIP67UDVX5UPXGN3XDAWVIEFBAVG6G7LUESEOU3NUQKTWN55W34YBG";
const CONSUMER = process.env.CONSUMER_ID || REFERENCE_CONSUMER;
const ADMIN_SECRET = process.env.ADMIN_SECRET;

if (!ADMIN_SECRET) {
  console.error("Set ADMIN_SECRET to a funded testnet secret (starts with 'S').");
  process.exit(1);
}

const server = new rpc.Server(RPC_URL);
const admin = Keypair.fromSecret(ADMIN_SECRET);
const account = await server.getAccount(admin.publicKey());

// 1. Fetch a fresh BTC/USD attestation. The SDK throws if it's >2s old.
const oracle = new Noeracle({ network: "testnet" });
const fresh = await oracle.fetchLatest(["BTC/USD"]);
const price = fresh.price("BTC/USD");
console.log(
  `[fetch ] BTC/USD = $${price.priceHuman.toFixed(2)}  ` +
    `(signed ${Math.floor(Date.now() / 1000) - price.timestamp}s ago, round ${price.roundId})`,
);

// 2. Build the consumer call. The six update_batch_ed25519_args ScVals come
//    from fresh.updateArgs(); we prepend the trader Address and position size.
const oracleArgs = fresh.updateArgs();
const trader = nativeToScVal(admin.publicKey(), { type: "address" });
const size = nativeToScVal(10_000_000n, { type: "i128" });

const op = new Contract(CONSUMER).call(
  "open_position",
  trader,
  size,
  ...oracleArgs,
);

const tx = new TransactionBuilder(account, {
  fee: "100000",
  networkPassphrase: NETWORK,
})
  .addOperation(op)
  .setTimeout(30)
  .build();

const prepared = await server.prepareTransaction(tx);
prepared.sign(admin);

// 3. Submit and wait for landing.
const sent = await server.sendTransaction(prepared);
console.log(`[submit] ${sent.hash} (${sent.status})`);

let landed;
for (let i = 0; i < 30; i++) {
  await new Promise((r) => setTimeout(r, 2000));
  landed = await server.getTransaction(sent.hash);
  if (landed.status !== "NOT_FOUND") break;
}

console.log(`[landed] ${landed.status}`);
if (landed.status === "SUCCESS" && landed.returnValue) {
  const position = scValToNative(landed.returnValue);
  console.log("[result] position:", JSON.stringify(position, (_, v) =>
    typeof v === "bigint" ? v.toString() : v, 2));
} else if (landed.status !== "SUCCESS") {
  console.error("[result]", JSON.stringify(landed, null, 2));
  process.exit(1);
}
