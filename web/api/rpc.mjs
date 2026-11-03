// Vercel serverless function: POST /api/rpc, read-only testnet relay (see ../rpc-allow.mjs).
// .mjs, not .js: web/ has no package.json, so a .js file would load as CommonJS and its import would crash.
import { relay, MAX_BODY } from "../rpc-allow.mjs";

export default async function handler(req, res) {
  res.setHeader("cache-control", "no-store");
  if (req.method !== "POST") { res.status(405).json({ error: "POST only" }); return; }
  const raw = typeof req.body === "string" ? req.body : JSON.stringify(req.body ?? null);
  if (raw.length > MAX_BODY) { res.status(413).json({ error: "body too large" }); return; }
  let body;
  try { body = JSON.parse(raw); } catch { res.status(400).json({ error: "bad JSON" }); return; }
  const out = await relay(body);
  res.status(out.status).json(out.json);
}
