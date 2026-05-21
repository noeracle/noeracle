// HTTP surface for the attestation service:
//   GET /health            — liveness + keeper stats
//   GET /v1/latest         — latest signed attestation for every asset
//   GET /v1/latest/:asset  — latest signed attestation for one asset
//                            (asset path uses '-' for '/', e.g. BTC-USD)

import http from "node:http";

export function createServer(keeper, startedAt) {
  return http.createServer((req, res) => {
    const url = new URL(req.url, "http://localhost");
    const send = (code, body) => {
      res.writeHead(code, {
        "content-type": "application/json",
        "access-control-allow-origin": "*",
      });
      res.end(JSON.stringify(body));
    };

    if (req.method !== "GET") return send(405, { error: "method not allowed" });

    if (url.pathname === "/health") {
      return send(200, {
        status: "ok",
        uptime_s: Math.floor((Date.now() - startedAt) / 1000),
        ...keeper.stats(),
      });
    }

    if (url.pathname === "/v1/latest") {
      return send(200, { assets: keeper.getAll() });
    }

    const match = url.pathname.match(/^\/v1\/latest\/(.+)$/);
    if (match) {
      const asset = decodeURIComponent(match[1]).replace("-", "/");
      const attestation = keeper.getLatest(asset);
      return attestation
        ? send(200, attestation)
        : send(404, { error: "no attestation for asset", asset });
    }

    send(404, { error: "not found" });
  });
}
