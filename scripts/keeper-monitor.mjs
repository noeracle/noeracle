// Noeracle keeper monitor.
//
// Runs on GitHub Actions — outside Fly — so it can still alert when the keeper
// or its machine is down. Polls the attestation service's /health and pushes
// to Telegram:
//
//   alert mode (every 10 min):       message only when the keeper is unhealthy
//   heartbeat  (every 6 h / manual): always send a status digest
//
// Because /health returns 503 when the keeper stalls, a single check — "is
// /health 200 with 4 assets live?" — catches a crashed process, a dead
// machine, a network failure, and a running-but-not-signing keeper alike.
//
// Local dry run (prints instead of sending; hits the live service):
//   DRY_RUN=1 node scripts/keeper-monitor.mjs

const MONITOR_URL = (process.env.MONITOR_URL || "https://noeracle.fly.dev")
  .replace(/\/+$/, "");
const BOT_TOKEN = process.env.TELEGRAM_BOT_TOKEN;
const CHAT_ID = process.env.TELEGRAM_CHAT_ID;
const DRY_RUN = !!process.env.DRY_RUN;
const RETRY_DELAY_MS = Number(process.env.RETRY_DELAY_MS || 10_000);
const ATTEMPTS = 3;

// Heartbeat on a manual run or the 6-hour cron; alert-only otherwise. With no
// GitHub env set (a local run) default to heartbeat so a dry run always prints.
const HEARTBEAT =
  process.env.GH_EVENT_NAME === "workflow_dispatch" ||
  process.env.GH_SCHEDULE === "5 */6 * * *" ||
  !process.env.GH_EVENT_NAME;

const sleep = (ms) => new Promise((r) => setTimeout(r, ms));

// Readable cause for a failed fetch (DNS, refused, timeout, ...).
function errText(err) {
  if (err.name === "TimeoutError") return "timeout";
  return err.cause?.code || err.cause?.message || err.message || String(err);
}

// One probe of /health — normalized result, never throws.
async function probe() {
  try {
    const res = await fetch(`${MONITOR_URL}/health`, {
      signal: AbortSignal.timeout(8000),
    });
    let body = null;
    try {
      body = await res.json();
    } catch {
      /* the keeper always sends JSON; ignore a malformed body */
    }
    return { reached: true, http: res.status, body };
  } catch (err) {
    return { reached: false, http: null, body: null, error: errText(err) };
  }
}

// Fully healthy: HTTP 200, the keeper reports it is signing, all 4 assets live.
function isHealthy(r) {
  return (
    r.reached &&
    r.http === 200 &&
    r.body != null &&
    r.body.status === "ok" &&
    r.body.assets_live === 4
  );
}

// Probe up to ATTEMPTS times so a single network blip never cries wolf; stop
// early on the first healthy probe.
async function checkHealth() {
  let last;
  for (let i = 1; i <= ATTEMPTS; i++) {
    last = await probe();
    if (isHealthy(last)) return { healthy: true, attempts: i, result: last };
    if (i < ATTEMPTS) await sleep(RETRY_DELAY_MS);
  }
  return { healthy: false, attempts: ATTEMPTS, result: last };
}

function humanDuration(s) {
  if (s == null || !Number.isFinite(s)) return "?";
  if (s >= 86400) return `${Math.floor(s / 86400)}d ${Math.floor((s % 86400) / 3600)}h`;
  if (s >= 3600) return `${Math.floor(s / 3600)}h ${Math.floor((s % 3600) / 60)}m`;
  if (s >= 60) return `${Math.floor(s / 60)}m`;
  return `${s}s`;
}

// Timestamp in the operator's local time (Istanbul, UTC+3).
function stamp() {
  const t = new Date().toLocaleString("en-GB", {
    timeZone: "Europe/Istanbul",
    day: "2-digit",
    month: "short",
    hour: "2-digit",
    minute: "2-digit",
  });
  return `${t} (UTC+3)`;
}

function fmtPrice(n) {
  if (!Number.isFinite(n)) return "?";
  if (n >= 100) return n.toLocaleString("en-US", { maximumFractionDigits: 0 });
  if (n >= 1) {
    return n.toLocaleString("en-US", {
      minimumFractionDigits: 2,
      maximumFractionDigits: 2,
    });
  }
  return n.toLocaleString("en-US", { maximumFractionDigits: 5 });
}

// The four live prices for the digest — best-effort, omitted on any failure.
async function priceLine() {
  try {
    const res = await fetch(`${MONITOR_URL}/v1/latest`, {
      signal: AbortSignal.timeout(8000),
    });
    if (!res.ok) return null;
    const { assets } = await res.json();
    const parts = Object.values(assets || {}).map(
      (a) => `${a.asset.split("/")[0]} ${fmtPrice(a.price_human)}`,
    );
    return parts.length ? parts.join(" · ") : null;
  } catch {
    return null;
  }
}

async function healthyMessage(body) {
  const prices = await priceLine();
  return [
    "✅ Noeracle healthy",
    `uptime ${humanDuration(body.uptime_s)} · ${body.assets_live}/4 assets · ` +
      `last signed ${body.last_signed_age_s ?? "?"}s`,
    `polls ${Number(body.polls).toLocaleString("en-US")}`,
    prices,
    stamp(),
  ]
    .filter(Boolean)
    .join("\n");
}

function problemMessage({ result: r, attempts }) {
  const lines = [];
  let head = "🔴 Noeracle DOWN";

  if (!r.reached) {
    lines.push(`${MONITOR_URL}/health unreachable`);
    lines.push(`error: ${r.error}`);
  } else if (r.http !== 200) {
    const status = r.body && r.body.status ? ` (${r.body.status})` : "";
    lines.push(`/health → HTTP ${r.http}${status}`);
    if (r.body && r.body.last_signed_age_s !== undefined) {
      lines.push(
        `last signed ${r.body.last_signed_age_s ?? "?"}s · ` +
          `${r.body.assets_live ?? "?"}/4 assets live`,
      );
    }
  } else {
    // HTTP 200 but not fully healthy — a partial degradation.
    head = "🟠 Noeracle degraded";
    lines.push(`/health OK but ${r.body?.assets_live ?? "?"}/4 assets live`);
    lines.push(`last signed ${r.body?.last_signed_age_s ?? "?"}s`);
  }
  lines.push(`checked ${attempts}× · ${stamp()}`);
  return [head, ...lines].join("\n");
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
    process.exit(1);
  }
  const res = await fetch(`https://api.telegram.org/bot${BOT_TOKEN}/sendMessage`, {
    method: "POST",
    headers: { "content-type": "application/json" },
    body: JSON.stringify({
      chat_id: CHAT_ID,
      text,
      disable_web_page_preview: true,
    }),
    signal: AbortSignal.timeout(10_000),
  });
  if (!res.ok) {
    const detail = await res.text().catch(() => "");
    console.error(`Telegram send failed: HTTP ${res.status} ${detail}`);
    process.exit(1);
  }
}

async function run() {
  const health = await checkHealth();
  if (!health.healthy) {
    await sendTelegram(problemMessage(health));
    console.log(`alert sent after ${health.attempts} attempts`);
  } else if (HEARTBEAT) {
    await sendTelegram(await healthyMessage(health.result.body));
    console.log("heartbeat digest sent");
  } else {
    console.log("keeper healthy — no message (alert mode)");
  }
}

run().catch((err) => {
  console.error("monitor error:", err.message || err);
  process.exit(1);
});
