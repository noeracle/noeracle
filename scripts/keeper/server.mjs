// HTTP + SSE surface for the attestation service:
//   GET /health            — liveness + keeper stats
//   GET /v1/latest         — latest signed attestation for every asset
//   GET /v1/latest/:asset  — latest signed attestation for one asset
//                            (asset path uses '-' for '/', e.g. BTC-USD)
//   GET /v1/stream         — Server-Sent Events: a `prices` event each round

import http from "node:http";

export function createServer(keeper, startedAt) {
  const sseClients = new Set();

  const httpServer = http.createServer((req, res) => {
    const url = new URL(req.url, "http://localhost");
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
          "GET /health": "liveness and keeper stats",
          "GET /v1/latest": "latest signed attestation for every asset",
          "GET /v1/latest/:asset": "one asset, e.g. /v1/latest/BTC-USD",
          "GET /v1/stream": "Server-Sent Events — a prices event each round",
        },
        repository: "https://github.com/y4hyya/Noeracle",
      });
    }

    if (url.pathname === "/health") {
      return sendJson(200, {
        status: "ok",
        uptime_s: Math.floor((Date.now() - startedAt) / 1000),
        sse_clients: sseClients.size,
        ...keeper.stats(),
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
      req.on("close", () => sseClients.delete(res));
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
