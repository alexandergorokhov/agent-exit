// Read-only NEAR testnet RPC relay used by the demo page's "Read live" button.
// The page's CSP allows only 'self', so the browser posts to /api/rpc and the
// relay forwards a small allow-list of view queries. Nothing can be signed or
// sent through it. Used by web/serve.mjs (local) and web/api/rpc.js (Vercel).

export const UPSTREAMS = ["https://rpc.testnet.fastnear.com", "https://rpc.testnet.near.org"];
export const VIEW_METHODS = ["get_status", "get_listing", "get_earnings"];
const ACCOUNT = /^[a-z0-9_-]+(\.[a-z0-9_-]+)*\.testnet$/;
export const MAX_BODY = 2048;

/** Returns null if the JSON-RPC body is an allowed read, else the reason. */
export function refuse(body) {
  if (!body || typeof body !== "object" || body.jsonrpc !== "2.0" || body.method !== "query") return "only JSON-RPC 'query' is relayed";
  const p = body.params;
  if (!p || typeof p !== "object" || p.finality !== "final") return "params.finality must be 'final'";
  if (typeof p.account_id !== "string" || p.account_id.length > 64 || !ACCOUNT.test(p.account_id)) return "account_id must be a .testnet account";
  const keys = Object.keys(p).sort().join(",");
  if (p.request_type === "view_account" || p.request_type === "view_access_key_list") {
    return keys === "account_id,finality,request_type" ? null : "unexpected params";
  }
  if (p.request_type === "call_function") {
    if (!VIEW_METHODS.includes(p.method_name)) return `method_name must be one of ${VIEW_METHODS.join(", ")}`;
    if (typeof p.args_base64 !== "string" || p.args_base64.length > 256) return "args_base64 too long";
    return keys === "account_id,args_base64,finality,method_name,request_type" ? null : "unexpected params";
  }
  return "request_type not allowed";
}

/** Forwards an allowed body to the first upstream that answers. */
export async function relay(body, upstreams = UPSTREAMS, fetchImpl = fetch) {
  const why = refuse(body);
  if (why) return { status: 400, json: { jsonrpc: "2.0", id: body && body.id, error: { message: `refused by relay: ${why}` } } };
  let last = "no upstream";
  for (const url of upstreams) {
    try {
      const r = await fetchImpl(url, { method: "POST", headers: { "content-type": "application/json" }, body: JSON.stringify(body), signal: AbortSignal.timeout(8000) });
      if (r.ok) return { status: 200, json: await r.json() };
      last = `${new URL(url).host}: HTTP ${r.status}`;
    } catch (e) { last = `${new URL(url).host}: ${e && e.message ? e.message : e}`; }
  }
  return { status: 502, json: { jsonrpc: "2.0", id: body.id, error: { message: `testnet RPC unreachable (${last})` } } };
}
