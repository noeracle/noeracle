// Fetch live Soroban network ConfigSettingEntry values for the limits that
// bound how much crypto we can stuff into one tx.
//
// Run:  node fetch_ledger_limits.mjs               # testnet (default)
//       NETWORK=mainnet node fetch_ledger_limits.mjs
//       NETWORK=both node fetch_ledger_limits.mjs

import { rpc, xdr } from "@stellar/stellar-sdk";

const RPC = {
  testnet: "https://soroban-testnet.stellar.org",
  mainnet: "https://mainnet.sorobanrpc.com",
};

// camelCase variant names matching xdr.ConfigSettingId static factory methods.
const CONFIG_VARIANTS = [
  "configSettingContractMaxSizeBytes",
  "configSettingContractComputeV0",
  "configSettingContractLedgerCostV0",
  "configSettingContractLedgerCostExtV0",
  "configSettingContractHistoricalDataV0",
  "configSettingContractEventsV0",
  "configSettingContractBandwidthV0",
  "configSettingContractDataKeySizeBytes",
  "configSettingContractDataEntrySizeBytes",
  "configSettingStateArchival",
  "configSettingContractExecutionLanes",
  "configSettingContractParallelComputeV0",
];

function buildKey(variantName) {
  const id = xdr.ConfigSettingId[variantName]();
  return xdr.LedgerKey.configSetting(
    new xdr.LedgerKeyConfigSetting({ configSettingId: id })
  );
}

async function fetchAll(network) {
  const url = RPC[network];
  if (!url) throw new Error(`unknown network: ${network}`);
  const server = new rpc.Server(url, { allowHttp: url.startsWith("http://") });

  const xdrKeys = CONFIG_VARIANTS.map(buildKey);
  const resp = await server.getLedgerEntries(...xdrKeys);

  const out = {};
  for (const entry of resp.entries) {
    const cs = entry.val.configSetting();
    out[cs.switch().name] = serialize(cs);
  }
  return { network, ledger: resp.latestLedger, settings: out };
}

function safeCall(obj, fn) {
  try {
    const v = obj[fn]?.();
    return v === undefined ? undefined : v.toString();
  } catch {
    return undefined;
  }
}

function serialize(cs) {
  const variant = cs.switch().name;
  switch (variant) {
    case "configSettingContractMaxSizeBytes":
      return { max_contract_size_bytes: cs.contractMaxSizeBytes().toString() };
    case "configSettingContractComputeV0": {
      const v = cs.contractCompute();
      return {
        ledger_max_instructions: v.ledgerMaxInstructions().toString(),
        tx_max_instructions: v.txMaxInstructions().toString(),
        fee_rate_per_instructions_increment: v.feeRatePerInstructionsIncrement().toString(),
        tx_memory_limit: v.txMemoryLimit().toString(),
      };
    }
    case "configSettingContractLedgerCostV0": {
      const v = cs.contractLedgerCost();
      const out = {};
      for (const fn of [
        "ledgerMaxDiskReadEntries", "ledgerMaxDiskReadBytes",
        "ledgerMaxWriteLedgerEntries", "ledgerMaxWriteBytes",
        "txMaxDiskReadEntries", "txMaxDiskReadBytes",
        "txMaxWriteLedgerEntries", "txMaxWriteBytes",
        "feeDiskReadLedgerEntry", "feeWriteLedgerEntry",
        "feeDiskRead1Kb", "sorobanStateTargetSizeBytes",
        "rentFee1KbSorobanStateSizeLow", "rentFee1KbSorobanStateSizeHigh",
        "sorobanStateRentFeeGrowthFactor",
      ]) {
        const r = safeCall(v, fn);
        if (r !== undefined) out[fn] = r;
      }
      return out;
    }
    case "configSettingContractLedgerCostExtV0": {
      const v = cs.contractLedgerCostExt();
      const out = {};
      for (const fn of ["txMaxFootprintEntries", "feeWrite1Kb"]) {
        const r = safeCall(v, fn);
        if (r !== undefined) out[fn] = r;
      }
      return out;
    }
    case "configSettingContractHistoricalDataV0":
      return { fee_historical1_kb: cs.contractHistoricalData().feeHistorical1Kb().toString() };
    case "configSettingContractEventsV0": {
      const v = cs.contractEvents();
      return {
        tx_max_contract_events_size_bytes: v.txMaxContractEventsSizeBytes().toString(),
        fee_contract_events1_kb: v.feeContractEvents1Kb().toString(),
      };
    }
    case "configSettingContractBandwidthV0": {
      const v = cs.contractBandwidth();
      return {
        ledger_max_txs_size_bytes: v.ledgerMaxTxsSizeBytes().toString(),
        tx_max_size_bytes: v.txMaxSizeBytes().toString(),
        fee_tx_size1_kb: v.feeTxSize1Kb().toString(),
      };
    }
    case "configSettingContractDataKeySizeBytes":
      return { max_contract_data_key_size_bytes: cs.contractDataKeySizeBytes().toString() };
    case "configSettingContractDataEntrySizeBytes":
      return { max_contract_data_entry_size_bytes: cs.contractDataEntrySizeBytes().toString() };
    case "configSettingStateArchival": {
      const v = cs.stateArchivalSettings();
      const out = {};
      for (const fn of [
        "maxEntryTtl", "minTemporaryTtl", "minPersistentTtl",
        "persistentRentRateDenominator", "tempRentRateDenominator",
        "maxEntriesToArchive",
        "liveSorobanStateSizeWindowSampleSize", "liveSorobanStateSizeWindowSamplePeriod",
        "evictionScanSize", "startingEvictionScanLevel",
      ]) {
        const r = safeCall(v, fn);
        if (r !== undefined) out[fn] = r;
      }
      return out;
    }
    case "configSettingContractExecutionLanes":
      return { ledger_max_tx_count: cs.contractExecutionLanes().ledgerMaxTxCount().toString() };
    case "configSettingContractParallelComputeV0":
      return { ledger_max_dependent_tx_clusters:
        safeCall(cs.contractParallelCompute(), "ledgerMaxDependentTxClusters") };
    default:
      return { raw: variant };
  }
}

function printTable(result) {
  console.log(`\n========== ${result.network.toUpperCase()} (ledger ${result.ledger}) ==========`);
  for (const [name, fields] of Object.entries(result.settings)) {
    console.log(`\n### ${name}`);
    for (const [k, v] of Object.entries(fields)) {
      if (v === undefined) continue;
      console.log(`  ${k.padEnd(50)} ${v}`);
    }
  }
}

const networkArg = process.env.NETWORK || "testnet";
const targets = networkArg === "both" ? ["testnet", "mainnet"] : [networkArg];

const results = [];
for (const net of targets) {
  try {
    const r = await fetchAll(net);
    printTable(r);
    results.push(r);
  } catch (e) {
    console.error(`!! ${net} failed: ${e.message}`);
  }
}

if (process.env.JSON === "1") {
  console.log("\n========== JSON ==========");
  console.log(JSON.stringify(results, null, 2));
}
