// Static server for the demo page: node web/serve.mjs [port]. No dependencies.
import { createServer } from "node:http";
import { readFile } from "node:fs/promises";
import { extname, join, normalize, sep } from "node:path";
import { fileURLToPath } from "node:url";
import { relay, MAX_BODY, UPSTREAMS } from "./rpc-allow.mjs";

const root = fileURLToPath(new URL(".", import.meta.url));
const types = { ".html": "text/html; charset=utf-8", ".js": "text/javascript; charset=utf-8", ".mjs": "text/javascript; charset=utf-8", ".css": "text/css; charset=utf-8", ".json": "application/json; charset=utf-8", ".svg": "image/svg+xml" };

// Same policy as the <meta> tag in index.html, plus frame-ancestors, which a
// meta tag cannot set. web/_headers and web/vercel.json must say the same.
export const CSP = "default-src 'none'; script-src 'self'; style-src 'self'; connect-src 'self'; img-src 'self'; base-uri 'none'; form-action 'none'; frame-ancestors 'none'";
export const SECURITY_HEADERS = { "content-security-policy": CSP, "x-content-type-options": "nosniff", "referrer-policy": "no-referrer" };

// `upstreams`: testnet RPC URLs for the read-only relay at /api/rpc
// (AGENT_EXIT_RPC overrides the first one). Tests pass their own.
export function serve(port = 8080, host = "127.0.0.1", upstreams = process.env.AGENT_EXIT_RPC ? [process.env.AGENT_EXIT_RPC, ...UPSTREAMS] : UPSTREAMS) {
  const server = createServer(async (req, res) => {
    let path;
    try { path = decodeURIComponent(new URL(req.url, "http://x").pathname); } catch { res.writeHead(400).end(); return; }
    if (path === "/api/rpc") { await rpcRoute(req, res, upstreams); return; }
    if (path.endsWith("/")) path += "index.html";
    const file = normalize(join(root, path));
    if (!file.startsWith(root) || file.includes(`${sep}test${sep}`)) { res.writeHead(404, SECURITY_HEADERS).end("not found"); return; }
    try {
      const body = await readFile(file);
      res.writeHead(200, { "content-type": types[extname(file)] || "application/octet-stream", "cache-control": "no-cache", ...SECURITY_HEADERS });
      res.end(body);
    } catch { res.writeHead(404, SECURITY_HEADERS).end("not found"); }
  });
  return new Promise((ok) => server.listen(port, host, () => ok(server)));
}

async function rpcRoute(req, res, upstreams) {
  const send = (status, json) => { res.writeHead(status, { "content-type": "application/json; charset=utf-8", "cache-control": "no-store", ...SECURITY_HEADERS }); res.end(JSON.stringify(json)); };
  if (req.method !== "POST") { send(405, { error: "POST only" }); return; }
  let raw = "";
  for await (const chunk of req) { raw += chunk; if (raw.length > MAX_BODY) { send(413, { error: "body too large" }); return; } }
  let body;
  try { body = JSON.parse(raw); } catch { send(400, { error: "bad JSON" }); return; }
  const out = await relay(body, upstreams);
  send(out.status, out.json);
}

if (process.argv[1] === fileURLToPath(import.meta.url)) {
  const port = Number(process.argv[2] || process.env.PORT || 8080);
  serve(port).then(() => console.log(`Agent Exit demo: http://127.0.0.1:${port}/  (testnet mode: ?network=testnet)`));
}
