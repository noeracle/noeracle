// Noeracle SLA check.
//
// Verifies the attestation service is meeting its v0 testnet SLA:
//   - every fetched attestation is signed within the last N seconds (default 2)
//   - every asset is aggregated from at least N exchanges (default 3)
//
// Runs from GitHub Actions on an hourly cron. Silent on success; posts to
// Telegram on any breach (single message summarising every failing asset).
//
// Local dry run (prints, does not send):
//   DRY_RUN=1 node scripts/sla_check.mjs

const SERVICE = (process.env.SERVICE_URL || "https://api.noeracle.org")
  .replace(/\/+$/, "");
const SLA_SECONDS = Number(process.env.SLA_SECONDS || 2);
const MIN_SOURCES = Number(process.env.MIN_SOURCES || 3);
const BOT_TOKEN = process.env.TELEGRAM_BOT_TOKEN;
const CHAT_ID = process.env.TELEGRAM_CHAT_ID;
const DRY_RUN = !!process.env.DRY_RUN;

const esc = (s) =>
  String(s).replace(/&/g, "&amp;").replace(/</g, "&lt;").replace(/>/g, "&gt;");

function stamp() {
  const t = new Date().toLocaleString("en-GB", {
    timeZone: "Europe/Istanbul",
    day: "numeric",
    month: "short",
    hour: "2-digit",
    minute: "2-digit",
  });
  return `${t.replace(",", "")} · UTC+3`;
}

function errText(err) {
  if (err.name === "TimeoutError") return "timeout";
  return err.cause?.code || err.cause?.message || err.message || String(err);
}

async function fetchLatest() {
  const res = await fetch(`${SERVICE}/v1/latest`, {
    signal: AbortSignal.timeout(8000),
  });
  if (!res.ok) throw new Error(`HTTP ${res.status}`);
  const body = await res.json();
  return body.assets || {};
}

function checkAsset(asset, now) {
  const age = now - asset.timestamp;
  const fails = [];
  if (age > SLA_SECONDS) {
    fails.push(`age ${age}s > ${SLA_SECONDS}s SLA`);
  }
  if (asset.sources < MIN_SOURCES) {
    fails.push(`only ${asset.sources} sources (min ${MIN_SOURCES})`);
  }
  return fails;
}

async function sendTelegram(text) {
  if (DRY_RUN) {
    console.log(`--- DRY_RUN: would send to Telegram ---\n${text}`);
    return;
  }
  if (!BOT_TOKEN || !CHAT_ID) {
    console.error(
      "TELEGRAM_BOT_TOKEN / TELEGRAM_CHAT_ID not set — add them as repo secrets.",
    );
    process.exit(2);
  }
  const res = await fetch(
    `https://api.telegram.org/bot${BOT_TOKEN}/sendMessage`,
    {
      method: "POST",
      headers: { "content-type": "application/json" },
      body: JSON.stringify({
        chat_id: CHAT_ID,
        text,
        parse_mode: "HTML",
        disable_web_page_preview: true,
      }),
      signal: AbortSignal.timeout(10_000),
    },
  );
  if (!res.ok) {
    const detail = await res.text().catch(() => "");
    console.error(`Telegram send failed: HTTP ${res.status} ${detail}`);
    process.exit(2);
  }
}

async function run() {
  let assets;
  try {
    assets = await fetchLatest();
  } catch (err) {
    await sendTelegram(
      [
        "🔴 <b>Noeracle SLA — service unreachable</b>",
        "",
        `Error — ${esc(errText(err))}`,
        `Endpoint — ${esc(SERVICE)}/v1/latest`,
        "",
        `<i>${stamp()}</i>`,
      ].join("\n"),
    );
    console.error(`fetch failed: ${errText(err)}`);
    process.exit(1);
  }

  const now = Math.floor(Date.now() / 1000);
  const all = Object.values(assets);
  const breaches = [];

  if (all.length === 0) {
    breaches.push("no assets reported");
  }

  for (const a of all) {
    const fails = checkAsset(a, now);
    if (fails.length) breaches.push(`${a.asset} — ${fails.join("; ")}`);
  }

  if (breaches.length === 0) {
    console.log(
      `SLA OK — ${all.length} asset(s), all within ${SLA_SECONDS}s freshness, ≥${MIN_SOURCES} sources`,
    );
    return;
  }

  await sendTelegram(
    [
      "🟠 <b>Noeracle SLA breach</b>",
      "",
      ...breaches.map((b) => `• ${esc(b)}`),
      "",
      `SLA — ≤${SLA_SECONDS}s freshness, ≥${MIN_SOURCES} sources per asset`,
      `Endpoint — ${esc(SERVICE)}/v1/latest`,
      "",
      `<i>${stamp()}</i>`,
    ].join("\n"),
  );
  console.error(`SLA breach: ${breaches.length} issue(s)`);
  process.exit(1);
}

run().catch((err) => {
  console.error("sla_check error:", err.message || err);
  process.exit(2);
});
