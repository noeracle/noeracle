// End-to-end fee + CPU measurement for the oracle_v0 contract on Stellar.
//
// Run:  NETWORK=testnet node run_oracle_bench.mjs
//       NETWORK=mainnet node run_oracle_bench.mjs
//
// Each scenario runs 5 times; median fee + CPU is reported.

import dotenv from "dotenv";
dotenv.config({ path: new URL("../.env", import.meta.url) });
import {
  Contract,
  Keypair,
  Networks,
  Operation,
  TransactionBuilder,
  authorizeEntry,
  rpc,
  xdr,
  Address,
  nativeToScVal,
  scValToNative,
} from "@stellar/stellar-sdk";
import { ed25519 } from "@noble/curves/ed25519";
import { secp256k1 } from "@noble/curves/secp256k1";
import { p256 } from "@noble/curves/p256";
import { bls12_381 as bls } from "@noble/curves/bls12-381";
import { sha256 } from "@noble/hashes/sha256";

const NETWORK = process.env.NETWORK || "testnet";
const cfg = {
  testnet: {
    rpc: process.env.TESTNET_RPC,
    passphrase: process.env.TESTNET_PASSPHRASE,
    contractId: process.env.TESTNET_ORACLE_V0,
  },
  mainnet: {
    rpc: process.env.MAINNET_RPC,
    passphrase: process.env.MAINNET_PASSPHRASE,
    contractId: process.env.MAINNET_ORACLE_V0,
  },
}[NETWORK];
if (!cfg?.contractId) throw new Error(`no contract id for ${NETWORK}`);

const server = new rpc.Server(cfg.rpc);
const adminKp = Keypair.fromSecret(process.env.NOERACLE_ADMIN_SECRET);
const contract = new Contract(cfg.contractId);

const RUNS_PER_SCENARIO = 5;

// ---------- helpers ----------

function bytesScVal(buf) {
  return xdr.ScVal.scvBytes(Buffer.from(buf));
}
function vecScVal(items) {
  return xdr.ScVal.scvVec(items);
}
function u64ScVal(n) {
  return nativeToScVal(BigInt(n), { type: "u64" });
}
function u32ScVal(n) {
  return nativeToScVal(n, { type: "u32" });
}
function i128ScVal(n) {
  return nativeToScVal(BigInt(n), { type: "i128" });
}
function addressScVal(pk) {
  return new Address(pk).toScVal();
}

function median(arr) {
  const s = [...arr].sort((a, b) => a - b);
  return s[Math.floor(s.length / 2)];
}
function fmt(n) {
  return typeof n === "number" ? n.toLocaleString() : String(n);
}

function buildMsg(asset8, price, ts, roundId) {
  const buf = Buffer.alloc(40);
  asset8.copy(buf, 0);
  // i128 big-endian — sign extension for negative not relevant (we use positive)
  const pb = Buffer.alloc(16);
  let v = BigInt(price);
  for (let i = 15; i >= 0; i--) {
    pb[i] = Number(v & 0xffn);
    v >>= 8n;
  }
  pb.copy(buf, 8);
  buf.writeBigUInt64BE(BigInt(ts), 24);
  buf.writeBigUInt64BE(BigInt(roundId), 32);
  return buf;
}

// ---------- sig generators ----------

function genEd25519Set(n, msg) {
  const sks = [];
  const pks = [];
  const sigs = [];
  for (let i = 0; i < n; i++) {
    const sk = ed25519.utils.randomPrivateKey();
    const pk = ed25519.getPublicKey(sk);
    const sig = ed25519.sign(msg, sk);
    sks.push(sk);
    pks.push(Buffer.from(pk));
    sigs.push(Buffer.from(sig));
  }
  return { sks, pks, sigs };
}

function genSecp256k1Set(n, msg) {
  const digest = sha256(msg);
  const pks = [];
  const sigs = [];
  const recids = [];
  for (let i = 0; i < n; i++) {
    const sk = secp256k1.utils.randomPrivateKey();
    const pk = secp256k1.getPublicKey(sk, false); // uncompressed 65 bytes
    const sig = secp256k1.sign(digest, sk, { lowS: true });
    pks.push(Buffer.from(pk));
    sigs.push(Buffer.from(sig.toCompactRawBytes()));
    recids.push(sig.recovery);
  }
  return { pks, sigs, recids };
}

// G1 / G2 serialization: noble uses (sign-bit-packed compressed) by default;
// pass `false` to get uncompressed point. Soroban expects 96B G1 / 192B G2,
// no tag byte.
function g1ToBytes(point) {
  return Buffer.from(point.toRawBytes(false)); // 96 bytes
}
function g2ToBytes(point) {
  return Buffer.from(point.toRawBytes(false)); // 192 bytes
}

const BLS_DST_G1 = new TextEncoder().encode("BLS_SIG_BLS12381G1_XMD:SHA-256_SSWU_RO_NUL_");

function hashToG1(msgBytes) {
  // Match Soroban's hash_to_g1 semantics. noble exposes hashToCurve helper.
  // Returns ProjectivePoint.
  const pt = bls.G1.hashToCurve(msgBytes, { DST: BLS_DST_G1 });
  return pt;
}

function genBlsAggregateSet(n, msg) {
  // Sig in G1, pubkey in G2.
  const h_msg = hashToG1(msg);
  // Each sk_i is a random Fr scalar.
  const sks = [];
  const pubkeys_g2 = [];
  let agg_sig_g1 = bls.G1.ProjectivePoint.ZERO;
  for (let i = 0; i < n; i++) {
    const sk_bytes = bls.utils.randomPrivateKey(); // 32 bytes
    const sk = bls.fields.Fr.fromBytes(sk_bytes);
    const pk_g2 = bls.G2.ProjectivePoint.BASE.multiply(sk);
    const sig_g1 = h_msg.multiply(sk);
    sks.push(sk);
    pubkeys_g2.push(pk_g2);
    agg_sig_g1 = agg_sig_g1.add(sig_g1);
  }
  const neg_g2_gen = bls.G2.ProjectivePoint.BASE.negate();
  return {
    agg_sig: g1ToBytes(agg_sig_g1),
    h_msg: g1ToBytes(h_msg),
    neg_g2_gen: g2ToBytes(neg_g2_gen),
    pubkeys: pubkeys_g2.map(g2ToBytes),
  };
}

// ---------- tx runner ----------

let cachedAccount = null;
async function refreshAccount() {
  cachedAccount = await server.getAccount(adminKp.publicKey());
}

async function runScenario(label, opBuilder, opts = {}) {
  const cpuRuns = [];
  const memRuns = [];
  const feeRuns = [];
  const totalRuns = [];

  for (let r = 0; r < (opts.runs ?? RUNS_PER_SCENARIO); r++) {
    await refreshAccount();
    const tx = new TransactionBuilder(cachedAccount, {
      fee: "100",
      networkPassphrase: cfg.passphrase,
    })
      .addOperation(opBuilder())
      .setTimeout(45)
      .build();

    let sim;
    try {
      sim = await server.simulateTransaction(tx);
    } catch (e) {
      console.error(`  [${label}] sim error: ${e.message}`);
      continue;
    }
    if ("error" in sim) {
      console.error(`  [${label}] sim error: ${sim.error}`);
      continue;
    }

    // CPU is in sim.transactionData.build().resources().instructions()
    let cpu = 0;
    let writeBytes = 0;
    let readBytes = 0;
    try {
      const sorobanData = sim.transactionData.build();
      const res = sorobanData.resources();
      cpu = Number(res.instructions());
      writeBytes = Number(res.writeBytes());
      readBytes = Number(res.diskReadBytes?.() ?? res.readBytes?.() ?? 0);
    } catch (e) {
      // fall through
    }
    cpuRuns.push(cpu);
    memRuns.push(writeBytes); // repurpose mem column as writeBytes

    if (opts.simulateOnly) {
      feeRuns.push(Number(sim.minResourceFee));
      continue;
    }

    let prepared;
    try {
      prepared = rpc.assembleTransaction(tx, sim).build();
    } catch (e) {
      console.error(`  [${label}] assemble error: ${e.message}`);
      continue;
    }
    prepared.sign(adminKp);

    let sendResp;
    try {
      sendResp = await server.sendTransaction(prepared);
    } catch (e) {
      console.error(`  [${label}] send error: ${e.message}`);
      continue;
    }
    if (sendResp.status === "ERROR") {
      console.error(`  [${label}] send ERROR: ${JSON.stringify(sendResp.errorResult)}`);
      continue;
    }

    let result;
    for (let attempts = 0; attempts < 30; attempts++) {
      await new Promise((r2) => setTimeout(r2, 1500));
      result = await server.getTransaction(sendResp.hash);
      if (result.status !== "NOT_FOUND" && result.status !== "PENDING") break;
    }
    if (result.status !== "SUCCESS") {
      console.error(`  [${label}] tx ${sendResp.hash} status: ${result.status}`);
      continue;
    }

    const feeCharged = Number(result.resultXdr.feeCharged().toString());
    const resourceFee = Number(sim.minResourceFee);
    feeRuns.push(resourceFee);
    totalRuns.push(feeCharged);
  }

  return {
    label,
    cpu: cpuRuns.length ? median(cpuRuns) : null,
    mem: memRuns.length ? median(memRuns) : null,
    resourceFee: feeRuns.length ? median(feeRuns) : null,
    totalFee: totalRuns.length ? median(totalRuns) : null,
    samples: cpuRuns.length,
  };
}

function summary(rows) {
  console.log("\n==================== RESULTS ====================");
  console.log(
    "scenario".padEnd(40),
    "samples".padEnd(8),
    "cpu".padEnd(14),
    "writeB".padEnd(8),
    "resFee".padEnd(12),
    "totalFee(stroop)".padEnd(18),
    "totalFee(XLM)"
  );
  for (const r of rows) {
    if (!r) continue;
    const xlm = r.totalFee ? (r.totalFee / 10_000_000).toFixed(7) : "—";
    console.log(
      r.label.padEnd(40),
      String(r.samples).padEnd(8),
      String(fmt(r.cpu)).padEnd(14),
      String(fmt(r.mem)).padEnd(10),
      String(fmt(r.resourceFee)).padEnd(12),
      String(fmt(r.totalFee)).padEnd(18),
      xlm
    );
  }
}

// ---------- scenarios ----------

const ASSET = Buffer.from("BTCUSD\0\0", "ascii"); // 8 bytes
const PRICE = 65432_1000000n; // 65432.10 * 1e7 precision
const TS = BigInt(Math.floor(Date.now() / 1000));
const ROUND = 1n;

function opEd25519Args(n) {
  const msg = buildMsg(ASSET, PRICE, TS, ROUND + BigInt(n));
  const { pks, sigs } = genEd25519Set(n, msg);
  return () =>
    contract.call(
      "update_ed25519_args",
      bytesScVal(ASSET),
      i128ScVal(PRICE),
      u64ScVal(TS),
      u64ScVal(ROUND + BigInt(n)),
      vecScVal(pks.map(bytesScVal)),
      vecScVal(sigs.map(bytesScVal))
    );
}

function opEd25519Persistent(n) {
  const r = ROUND + 100n + BigInt(n);
  const msg = buildMsg(ASSET, PRICE, TS, r);
  const { pks, sigs } = genEd25519Set(n, msg);
  return () =>
    contract.call(
      "update_ed25519_persistent",
      bytesScVal(ASSET),
      i128ScVal(PRICE),
      u64ScVal(TS),
      u64ScVal(r),
      vecScVal(pks.map(bytesScVal)),
      vecScVal(sigs.map(bytesScVal))
    );
}

function opSecp256k1(n) {
  const r = ROUND + 200n + BigInt(n);
  const msg = buildMsg(ASSET, PRICE, TS, r);
  const { pks, sigs, recids } = genSecp256k1Set(n, msg);
  return () =>
    contract.call(
      "update_secp256k1",
      bytesScVal(ASSET),
      i128ScVal(PRICE),
      u64ScVal(TS),
      u64ScVal(r),
      vecScVal(pks.map(bytesScVal)),
      vecScVal(sigs.map(bytesScVal)),
      vecScVal(recids.map(u32ScVal))
    );
}

function opBlsAggregate(n) {
  const r = ROUND + 300n + BigInt(n);
  const msg = buildMsg(ASSET, PRICE, TS, r);
  const set = genBlsAggregateSet(n, msg);
  return () =>
    contract.call(
      "update_bls_agg",
      bytesScVal(ASSET),
      i128ScVal(PRICE),
      u64ScVal(TS),
      u64ScVal(r),
      bytesScVal(set.agg_sig),
      bytesScVal(set.h_msg),
      bytesScVal(set.neg_g2_gen),
      vecScVal(set.pubkeys.map(bytesScVal))
    );
}

function opGetPrice() {
  return () => contract.call("get_price", bytesScVal(ASSET));
}
function opGetPricePersistent() {
  return () => contract.call("get_price_pers", bytesScVal(ASSET));
}

// ---------- Auth-entry scenario (Item 3) ----------
//
// Each publisher Address calls require_auth() inside the contract. We
// generate N keypairs (not funded — they don't transact, only sign the
// auth payload), simulate to get the unsigned auth entries, sign them, then
// re-submit with the signed entries attached to the op.

// 5 reusable publisher keypairs — funded once at script start.
let PUBLISHER_KPS = null;

async function ensurePublisherAccounts(count) {
  if (PUBLISHER_KPS && PUBLISHER_KPS.length >= count) return;
  console.log(`Setting up ${count} publisher accounts...`);
  PUBLISHER_KPS = Array.from({ length: count }, () => Keypair.random());

  if (NETWORK === "testnet") {
    // friendbot-fund each
    for (const kp of PUBLISHER_KPS) {
      const url = `https://friendbot.stellar.org/?addr=${kp.publicKey()}`;
      const resp = await fetch(url);
      if (!resp.ok) {
        console.error(`  friendbot failed for ${kp.publicKey()}: ${resp.status}`);
      } else {
        console.log(`  funded ${kp.publicKey()}`);
      }
    }
  } else {
    // mainnet: send 2 XLM from admin to each
    const minBalance = 2;
    for (const kp of PUBLISHER_KPS) {
      await refreshAccount();
      const tx = new TransactionBuilder(cachedAccount, {
        fee: "100",
        networkPassphrase: cfg.passphrase,
      })
        .addOperation(
          Operation.createAccount({
            destination: kp.publicKey(),
            startingBalance: String(minBalance),
          })
        )
        .setTimeout(45)
        .build();
      tx.sign(adminKp);
      const sendResp = await server.sendTransaction(tx);
      console.log(`  create_account ${kp.publicKey()}: ${sendResp.status}`);
      let result;
      for (let a = 0; a < 30; a++) {
        await new Promise((r2) => setTimeout(r2, 1500));
        result = await server.getTransaction(sendResp.hash);
        if (result.status !== "NOT_FOUND" && result.status !== "PENDING") break;
      }
    }
  }
  // brief settle delay
  await new Promise((r) => setTimeout(r, 3000));
}

async function runAuthEntryScenario(n) {
  await ensurePublisherAccounts(Math.max(5, n));
  const cpuRuns = [];
  const memRuns = [];
  const feeRuns = [];
  const totalRuns = [];
  const label = `auth-entry x${n}`;

  for (let r = 0; r < RUNS_PER_SCENARIO; r++) {
    const pubKps = PUBLISHER_KPS.slice(0, n);
    const round = ROUND + 500n + BigInt(n * 1000 + r);

    const args = [
      bytesScVal(ASSET),
      i128ScVal(PRICE),
      u64ScVal(TS),
      u64ScVal(round),
      vecScVal(pubKps.map((kp) => addressScVal(kp.publicKey()))),
    ];

    const invokeArgs = new xdr.InvokeContractArgs({
      contractAddress: new Address(cfg.contractId).toScAddress(),
      functionName: "update_via_auth",
      args,
    });
    const hostFn = xdr.HostFunction.hostFunctionTypeInvokeContract(invokeArgs);

    const buildOp = (auths) => Operation.invokeHostFunction({ func: hostFn, auth: auths ?? [] });

    // Pass 1: simulate without auths to find what auths are required
    await refreshAccount();
    let tx = new TransactionBuilder(cachedAccount, {
      fee: "100",
      networkPassphrase: cfg.passphrase,
    })
      .addOperation(buildOp())
      .setTimeout(45)
      .build();

    let sim;
    try {
      sim = await server.simulateTransaction(tx);
    } catch (e) {
      console.error(`  [${label}] sim1 error: ${e.message}`);
      continue;
    }
    if ("error" in sim) {
      console.error(`  [${label}] sim1 error: ${sim.error}`);
      continue;
    }

    // The sim returns the required auth entries on sim.result.auth
    const required = sim.result?.auth ?? [];
    if (required.length === 0) {
      console.error(`  [${label}] sim returned 0 required auths (expected ${n})`);
      continue;
    }

    // Sign each
    const validUntil = sim.latestLedger + 100;
    const signed = [];
    for (const entry of required) {
      // Match entry's credentials → keypair
      let signer;
      try {
        const credAddrXdr = entry.credentials().address().address();
        // ScAddress → string
        const matchedAddr = Address.fromScAddress(credAddrXdr).toString();
        signer = pubKps.find((kp) => kp.publicKey() === matchedAddr);
      } catch (e) {
        // fallback to first kp
        signer = pubKps[0];
      }
      if (!signer) {
        console.error(`  [${label}] could not match auth entry to keypair`);
        continue;
      }
      const signedEntry = await authorizeEntry(entry, signer, validUntil, cfg.passphrase);
      signed.push(signedEntry);
    }

    if (signed.length !== required.length) {
      console.error(`  [${label}] only signed ${signed.length}/${required.length}`);
      continue;
    }

    // Pass 2: rebuild tx with signed auths, re-simulate (resources may grow)
    await refreshAccount();
    tx = new TransactionBuilder(cachedAccount, {
      fee: "100",
      networkPassphrase: cfg.passphrase,
    })
      .addOperation(buildOp(signed))
      .setTimeout(45)
      .build();

    try {
      sim = await server.simulateTransaction(tx);
    } catch (e) {
      console.error(`  [${label}] sim2 error: ${e.message}`);
      continue;
    }
    if ("error" in sim) {
      console.error(`  [${label}] sim2 error: ${sim.error}`);
      continue;
    }

    let cpu = 0;
    let writeBytes = 0;
    try {
      const res = sim.transactionData.build().resources();
      cpu = Number(res.instructions());
      writeBytes = Number(res.writeBytes());
    } catch (e) {}
    cpuRuns.push(cpu);
    memRuns.push(writeBytes);

    let prepared;
    try {
      prepared = rpc.assembleTransaction(tx, sim).build();
    } catch (e) {
      console.error(`  [${label}] assemble error: ${e.message}`);
      continue;
    }
    prepared.sign(adminKp);

    const sendResp = await server.sendTransaction(prepared);
    if (sendResp.status === "ERROR") {
      console.error(`  [${label}] send ERROR: ${JSON.stringify(sendResp.errorResult)}`);
      continue;
    }

    let result;
    for (let a = 0; a < 30; a++) {
      await new Promise((r2) => setTimeout(r2, 1500));
      result = await server.getTransaction(sendResp.hash);
      if (result.status !== "NOT_FOUND" && result.status !== "PENDING") break;
    }
    if (result.status !== "SUCCESS") {
      console.error(`  [${label}] tx ${sendResp.hash} status: ${result.status}`);
      continue;
    }

    feeRuns.push(Number(sim.minResourceFee));
    totalRuns.push(Number(result.resultXdr.feeCharged().toString()));
  }

  return {
    label,
    cpu: cpuRuns.length ? median(cpuRuns) : null,
    mem: memRuns.length ? median(memRuns) : null,
    resourceFee: feeRuns.length ? median(feeRuns) : null,
    totalFee: totalRuns.length ? median(totalRuns) : null,
    samples: cpuRuns.length,
  };
}

// ---------- main ----------

async function main() {
  const results = [];

  // Set publishers once so update_ed25519_stored works
  {
    const msg = buildMsg(ASSET, PRICE, TS, ROUND);
    const { pks } = genEd25519Set(5, msg);
    console.log("Initializing publishers (5 keys)...");
    await runScenario("init publishers (5)", () =>
      contract.call("set_publishers", vecScVal(pks.map(bytesScVal)))
    , { runs: 1 });
  }

  for (const n of [1, 3, 5, 7, 10]) {
    results.push(await runScenario(`ed25519 args x${n}`, opEd25519Args(n)));
  }
  results.push(await runScenario(`ed25519 persistent x5`, opEd25519Persistent(5)));
  for (const n of [1, 3, 5, 7, 10]) {
    results.push(await runScenario(`secp256k1 x${n}`, opSecp256k1(n)));
  }
  for (const n of [1, 3, 5, 7, 10]) {
    results.push(await runScenario(`bls aggregate x${n}`, opBlsAggregate(n)));
  }
  results.push(await runScenario("get_price (temp)", opGetPrice()));
  results.push(await runScenario("get_price (pers)", opGetPricePersistent()));

  // Item 3: auth-entry path
  results.push(await runAuthEntryScenario(1));
  results.push(await runAuthEntryScenario(3));
  results.push(await runAuthEntryScenario(5));

  summary(results);
}

main().catch((e) => {
  console.error(e);
  process.exit(1);
});
