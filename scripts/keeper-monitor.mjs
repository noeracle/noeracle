// Noeracle keeper monitor.
//
// Runs on GitHub Actions — outside Fly — so it can still alert when the keeper
// or its machine is down. Polls the attestation service's /health and pushes
// to Telegram:
//
//   alert mode (every 10 min):        message only when the keeper is unhealthy
//   heartbeat  (once a day / manual): always send a status digest
//
// Because /health returns 503 when the keeper stalls, a single check — "is
// /health 200 with 4 assets live?" — catches a crashed process, a dead
// machine, a network failure, and a running-but-not-signing keeper alike.
//
// Local dry run (prints instead of sending; hits the live service):
//   DRY_RUN=1 node scripts/keeper-monitor.mjs

const MONITOR_URL = (process.env.MONITOR_URL || "https://api.noeracle.org")
  .replace(/\/+$/, "");
const BOT_TOKEN = process.env.TELEGRAM_BOT_TOKEN;
const CHAT_ID = process.env.TELEGRAM_CHAT_ID;
const DRY_RUN = !!process.env.DRY_RUN;
const RETRY_DELAY_MS = Number(process.env.RETRY_DELAY_MS || 10_000);
const ATTEMPTS = 3;

// Heartbeat on a manual run or the daily digest cron; alert-only on the 10-min
// check. Keying off "a scheduled run that is NOT the alert cron" lets the digest
// time change in keeper-monitor.yml without editing this file. With no GitHub
// env set (a local run) default to heartbeat so a dry run always prints.
const ALERT_CRON = "*/10 * * * *";
const HEARTBEAT =
  process.env.GH_EVENT_NAME === "workflow_dispatch" ||
  (process.env.GH_EVENT_NAME === "schedule" &&
    process.env.GH_SCHEDULE !== ALERT_CRON) ||
  !process.env.GH_EVENT_NAME;

const sleep = (ms) => new Promise((r) => setTimeout(r, ms));

// Readable cause for a failed fetch (DNS, refused, timeout, ...).
function errText(err) {
  if (err.name === "TimeoutError") return "timeout";
  return err.cause?.code || err.cause?.message || err.message || String(err);
}

// Escape the few free-text fields that go into an HTML-parsed message.
const esc = (s) =>
  String(s).replace(/&/g, "&amp;").replace(/</g, "&lt;").replace(/>/g, "&gt;");

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

// Fully healthy: HTTP 200 and the keeper reports it is signing. The keeper's
// own /health endpoint returns 503 when staleness exceeds HEALTH_STALENESS_S,
// so HTTP 200 + status==="ok" is sufficient — no need to anchor to a specific
// asset count, which would false-positive every time we add a new asset.
function isHealthy(r) {
  return (
    r.reached &&
    r.http === 200 &&
    r.body != null &&
    r.body.status === "ok"
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
    day: "numeric",
    month: "short",
    hour: "2-digit",
    minute: "2-digit",
  });
  // en-GB renders "22 May, 18:21" — drop the comma for a cleaner footer.
  return `${t.replace(",", "")} · UTC+3`;
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
      (a) => `${a.asset.split("/")[0]} $${fmtPrice(a.price_human)}`,
    );
    return parts.length ? parts.join(" · ") : null;
  } catch {
    return null;
  }
}

async function healthyMessage(body) {
  const prices = await priceLine();
  const lines = [
    "✅ <b>Noeracle healthy</b>",
    "",
    `Uptime — ${humanDuration(body.uptime_s)}`,
    `Assets — ${body.assets_live} live`,
    `Last signed — ${body.last_signed_age_s ?? "?"}s ago`,
    `Polls — ${Number(body.polls).toLocaleString("en-US")}`,
  ];
  if (prices) lines.push("", prices);
  lines.push("", `<i>${stamp()}</i>`);
  return lines.join("\n");
}

function problemMessage({ result: r, attempts }) {
  let head = "🔴 <b>Noeracle DOWN</b>";
  const lines = [];

  if (!r.reached) {
    lines.push("Health — unreachable");
    lines.push(`Error — ${esc(r.error)}`);
  } else if (r.http !== 200) {
    const status = r.body && r.body.status ? ` (${esc(r.body.status)})` : "";
    lines.push(`Health — HTTP ${r.http}${status}`);
    if (r.body && r.body.last_signed_age_s !== undefined) {
      lines.push(`Last signed — ${r.body.last_signed_age_s ?? "?"}s ago`);
      lines.push(`Assets — ${r.body.assets_live ?? "?"} live`);
    }
  } else {
    // HTTP 200 but not fully healthy — a partial degradation.
    head = "🟠 <b>Noeracle degraded</b>";
    lines.push(`Health — HTTP 200, ${r.body?.assets_live ?? "?"} assets live`);
    lines.push(`Last signed — ${r.body?.last_signed_age_s ?? "?"}s ago`);
  }
  lines.push(`Checks — ${attempts} attempts`);

  return [head, "", ...lines, "", `<i>${stamp()}</i>`].join("\n");
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
      parse_mode: "HTML",
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
