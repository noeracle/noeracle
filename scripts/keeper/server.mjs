// HTTP + SSE surface for the attestation service:
//   GET /health            — liveness + staleness (HTTP 503 when stalled)
//   GET /v1/latest         — latest signed attestation for every asset
//   GET /v1/latest/:asset  — latest signed attestation for one asset
//                            (asset path uses '-' for '/', e.g. BTC-USD)
//   GET /v1/stream         — Server-Sent Events: a `prices` event each round

import http from "node:http";

import { HEALTH_STALENESS_S, SERVE_MAX_STALENESS_S } from "./config.mjs";

export function createServer(keeper, startedAt) {
  const sseClients = new Set();

  const httpServer = http.createServer((req, res) => {
    const url = new URL(req.url, "http://localhost");

    // Access log — one line per request once the response finishes. /health
    // is skipped; Fly and the uptime monitor poll it constantly.
    const reqStart = Date.now();
    res.on("finish", () => {
      if (url.pathname !== "/health") {
        console.log(
          `[req] ${req.method} ${url.pathname} → ${res.statusCode} ` +
            `${Date.now() - reqStart}ms`,
        );
      }
    });

    const sendJson = (code, body) => {
      res.writeHead(code, {
        "content-type": "application/json",
        "access-control-allow-origin": "*",
      });
      res.end(JSON.stringify(body));
    };

    if (req.method !== "GET") return sendJson(405, { error: "method not allowed" });

    if (url.pathname === "/") {
      return sendJson(200, {
        service: "Noeracle attestation service",
        description:
          "Pull-based price oracle for Stellar — fetch a freshly signed " +
          "price and verify it on-chain.",
        endpoints: {
          "GET /health": "liveness and staleness — HTTP 503 when not signing",
          "GET /v1/latest": "latest signed attestation for every asset",
          "GET /v1/latest/:asset": "one asset, e.g. /v1/latest/BTC-USD",
          "GET /v1/stream": "Server-Sent Events — a prices event each round",
        },
        repository: "https://github.com/noeracle/noeracle",
      });
    }

    if (url.pathname === "/health") {
      const stats = keeper.stats();
      const age = stats.last_signed_age_s;
      // Liveness is decoupled from freshness. While the keeper holds a price
      // signed within SERVE_MAX_STALENESS_S we return 200 and stay in the Fly
      // load balancer — a pull oracle serving a slightly-stale but still
      // cryptographically valid price beats serving nothing. We fail the check
      // (503 → out of rotation; the watchdog then forces a fresh-IP restart)
      // only when the keeper has never signed or is stale past the point a
      // price is even usable on-chain.
      const serving = age !== null && age <= SERVE_MAX_STALENESS_S;
      const fresh = age !== null && age <= HEALTH_STALENESS_S;
      const status = !serving ? "stalled" : fresh ? "ok" : "degraded";
      return sendJson(serving ? 200 : 503, {
        status,
        fresh,
        uptime_s: Math.floor((Date.now() - startedAt) / 1000),
        sse_clients: sseClients.size,
        ...stats,
      });
    }

    if (url.pathname === "/v1/latest") {
      return sendJson(200, { assets: keeper.getAll() });
    }

    if (url.pathname === "/v1/stream") {
      res.writeHead(200, {
        "content-type": "text/event-stream",
        "cache-control": "no-cache",
        connection: "keep-alive",
        "access-control-allow-origin": "*",
      });
      // Send the current snapshot immediately, then stream every round.
      res.write(`event: prices\ndata: ${JSON.stringify({ assets: keeper.getAll() })}\n\n`);
      sseClients.add(res);
      console.log(`[sse] connect — ${sseClients.size} client(s)`);
      req.on("close", () => {
        sseClients.delete(res);
        console.log(`[sse] disconnect — ${sseClients.size} client(s)`);
      });
      return;
    }

    const match = url.pathname.match(/^\/v1\/latest\/(.+)$/);
    if (match) {
      const asset = decodeURIComponent(match[1]).replace("-", "/");
      const attestation = keeper.getLatest(asset);
      return attestation
        ? sendJson(200, attestation)
        : sendJson(404, { error: "no attestation for asset", asset });
    }

    sendJson(404, { error: "not found" });
  });

  // Push the latest attestations to every connected SSE client.
  function broadcast() {
    if (sseClients.size === 0) return;
    const payload = `event: prices\ndata: ${JSON.stringify({ assets: keeper.getAll() })}\n\n`;
    for (const res of sseClients) {
      try {
        res.write(payload);
      } catch {
        sseClients.delete(res);
      }
    }
  }

  return { httpServer, broadcast };
}
