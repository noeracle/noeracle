// Noeracle end-to-end demo — proves the full pull-oracle path on Stellar
// testnet:
//
//   1. the SDK fetches a freshly signed price from the attestation service
//   2. it builds the update_batch_ed25519_args verification operation
//   3. the operation is submitted on testnet — the contract verifies the
//      publisher signature and stores the price
//   4. a get_price read confirms the verified price is on-chain
//
// Start a local attestation service first:
//   cd scripts && npm run keeper
// then, from the repo root:
//   node sdk/examples/e2e.mjs

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
import { Noeracle } from "../dist/index.js";

// --- config: read what we need from the repo-root .env ------------------
const env = Object.fromEntries(
  readFileSync(new URL("../../.env", import.meta.url), "utf8")
    .split("\n")
    .filter((l) => l && !l.startsWith("#") && l.includes("="))
    .map((l) => {
      const i = l.indexOf("=");
      return [l.slice(0, i).trim(), l.slice(i + 1).trim().replace(/^"|"$/g, "")];
    }),
);
const CONTRACT_ID = env.TESTNET_ORACLE_V0;
const SOURCE = Keypair.fromSecret(env.NOERACLE_ADMIN_SECRET);
const KEEPER_URL = process.env.KEEPER_URL || "https://api.noeracle.org";
const server = new rpc.Server(env.TESTNET_RPC || "https://soroban-testnet.stellar.org");

async function main() {
  // === The entire Noeracle consumer surface — fewer than 10 lines. ======
  const oracle = new Noeracle({ network: "testnet", attestationUrl: KEEPER_URL });
  const fresh = await oracle.fetchLatest(["BTC/USD"]);
  const updateOp = fresh.toUpdateOp(CONTRACT_ID);
  // ======================================================================

  const fetched = fresh.price("BTC/USD");
  console.log(
    `fetched signed price:  BTC/USD = ${fetched.priceHuman}  (round ${fetched.roundId})`,
  );

  // Submit the verification operation on testnet.
  const account = await server.getAccount(SOURCE.publicKey());
  let tx = new TransactionBuilder(account, {
    fee: "1000000",
    networkPassphrase: Networks.TESTNET,
  })
    .addOperation(updateOp)
    .setTimeout(60)
    .build();
  tx = await server.prepareTransaction(tx);
  tx.sign(SOURCE);

  const sent = await server.sendTransaction(tx);
  if (sent.status === "ERROR") {
    throw new Error(`submit failed: ${JSON.stringify(sent.errorResult)}`);
  }
  console.log(`submitted update tx:   ${sent.hash}`);

  let result;
  for (let i = 0; i < 40; i++) {
    await new Promise((r) => setTimeout(r, 1500));
    result = await server.getTransaction(sent.hash);
    if (result.status !== "NOT_FOUND") break;
  }
  if (!result || result.status !== "SUCCESS") {
    throw new Error(`update tx did not succeed: ${result && result.status}`);
  }
  console.log(`update tx landed:      SUCCESS`);

  // Read get_price back from the contract to confirm it is stored on-chain.
  const readTx = new TransactionBuilder(
    await server.getAccount(SOURCE.publicKey()),
    { fee: "1000000", networkPassphrase: Networks.TESTNET },
  )
    .addOperation(
      new Contract(CONTRACT_ID).call(
        "get_price",
        xdr.ScVal.scvBytes(Buffer.from("BTCUSD\0\0", "ascii")),
      ),
    )
    .setTimeout(60)
    .build();
  const sim = await server.simulateTransaction(readTx);
  if (sim.error) throw new Error(`get_price simulation failed: ${sim.error}`);
  if (!sim.result || !sim.result.retval) throw new Error("get_price returned nothing");
  const onChain = scValToNative(sim.result.retval);
  console.log(
    `on-chain get_price:    price=${onChain.price}  round=${onChain.round_id}`,
  );

  if (BigInt(onChain.price) === fetched.price) {
    console.log(
      "\n✅ end-to-end verified — the SDK-fetched signed price was " +
        "verified by the contract and stored on Stellar testnet.",
    );
  } else {
    throw new Error("on-chain price does not match the fetched price");
  }
}

main().catch((err) => {
  console.error(`\n❌ ${err.message}`);
  if (String(err.message).toLowerCase().includes("unreachable")) {
    console.error("   is the attestation service running? cd scripts && npm run keeper");
  }
  process.exit(1);
});
