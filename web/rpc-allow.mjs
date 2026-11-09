// Read-only NEAR testnet RPC relay used by the demo page's "Read live" button.
// The page's CSP allows only 'self', so the browser posts to /api/rpc and the
// relay forwards a small allow-list of view queries and finished-transaction lookups. Nothing can be signed or
// sent through it. Used by web/serve.mjs (local) and web/api/rpc.mjs (Vercel).

export const UPSTREAMS = ["https://rpc.testnet.fastnear.com", "https://rpc.testnet.near.org"];
// Regular nodes forget transactions after a few days; "tx" falls back to an archival node.
export const ARCHIVAL = "https://archival-rpc.testnet.fastnear.com";
export const VIEW_METHODS = ["get_status", "get_listing", "get_earnings"];
const ACCOUNT = /^([a-z0-9_-]+(\.[a-z0-9_-]+)*\.testnet|[0-9a-f]{64})$/;
export const MAX_BODY = 2048;

/** Returns null if the JSON-RPC body is an allowed read, else the reason. */
const TX_HASH = /^[1-9A-HJ-NP-Za-km-z]{43,44}$/;

export function refuse(body) {
  // "tx": read a finished transaction's outcome (the page's "Verify on chain"). Read-only.
  if (body && typeof body === "object" && body.jsonrpc === "2.0" && body.method === "tx") {
    const p = body.params;
    if (!p || typeof p !== "object" || Array.isArray(p)) return "tx params must be an object";
    if (Object.keys(p).sort().join(",") !== "sender_account_id,tx_hash,wait_until") return "unexpected params";
    if (typeof p.tx_hash !== "string" || !TX_HASH.test(p.tx_hash)) return "tx_hash must be base58";
    if (typeof p.sender_account_id !== "string" || p.sender_account_id.length > 64 || !/^[a-z0-9_-]+(\.[a-z0-9_-]+)*\.testnet$/.test(p.sender_account_id)) return "sender_account_id must be a .testnet account";
    if (p.wait_until !== "NONE") return "wait_until must be 'NONE' (finished transactions only)";
    return null;
  }
  if (!body || typeof body !== "object" || body.jsonrpc !== "2.0" || body.method !== "query") return "only JSON-RPC 'query' and 'tx' are relayed";
  const p = body.params;
  if (!p || typeof p !== "object" || p.finality !== "final") return "params.finality must be 'final'";
  if (typeof p.account_id !== "string" || p.account_id.length > 64 || !ACCOUNT.test(p.account_id)) return "account_id must be a .testnet account";
  const keys = Object.keys(p).sort().join(",");
  // view_code: the "Check any NEAR account" box lists the contract's methods.
  if (p.request_type === "view_account" || p.request_type === "view_access_key_list" || p.request_type === "view_code") {
    return keys === "account_id,finality,request_type" ? null : "unexpected params";
  }
  if (p.request_type === "call_function") {
    if (!VIEW_METHODS.includes(p.method_name)) return `method_name must be one of ${VIEW_METHODS.join(", ")}`;
    if (typeof p.args_base64 !== "string" || p.args_base64.length > 256) return "args_base64 too long";
    return keys === "account_id,args_base64,finality,method_name,request_type" ? null : "unexpected params";
  }
  return "request_type not allowed";
}

// Review 5, M6: judges may all press "Run and verify" at once. A finished
// transaction never changes, so its answer is cached for the life of the
// function instance; a view query is cached for 10 s. Errors are never cached.
const CACHE = new Map();
const CACHE_MAX = 500;
const QUERY_TTL_MS = 10_000;
const UPSTREAM_TIMEOUT_MS = 4500;

export function clearCache() { CACHE.clear(); }

function cacheKey(body) { return JSON.stringify([body.method, body.params]); }

function cached(body) {
  const hit = CACHE.get(cacheKey(body));
  if (!hit || (hit.expires && hit.expires < Date.now())) return null;
  return { ...hit.json, id: body.id };
}

// Review 4 L6: contract code can be megabytes; refuse big answers and never
// cache code, so 500 entries cannot exhaust the function's memory.
const MAX_CODE_B64 = 1_400_000;

function remember(body, json) {
  if (!json || json.error || json.result === undefined) return;
  if (body.params && body.params.request_type === "view_code") return;
  if (CACHE.size >= CACHE_MAX) CACHE.delete(CACHE.keys().next().value);
  CACHE.set(cacheKey(body), { json, expires: body.method === "tx" ? 0 : Date.now() + QUERY_TTL_MS });
}

const short = (s) => String(s).replace(/[^ -~]/g, " ").slice(0, 120);

/** Forwards an allowed body to the first upstream that answers. */
export async function relay(body, upstreams = UPSTREAMS, fetchImpl = fetch) {
  const why = refuse(body);
  if (why) return { status: 400, json: { jsonrpc: "2.0", id: body && body.id, error: { message: `refused by relay: ${why}` } } };
  const hit = cached(body);
  if (hit) return { status: 200, json: hit };
  let last = "no upstream";
  const urls = body.method === "tx" ? [...upstreams, ARCHIVAL] : upstreams;
  for (const url of urls) {
    try {
      const r = await fetchImpl(url, { method: "POST", headers: { "content-type": "application/json" }, body: JSON.stringify(body), redirect: "error", signal: AbortSignal.timeout(UPSTREAM_TIMEOUT_MS) });
      if (r.ok) {
        const j = await r.json();
        const unknown = body.method === "tx" && j && j.error && /UNKNOWN_TRANSACTION|doesn't exist|garbage/i.test(JSON.stringify(j.error));
        if (unknown && url !== urls[urls.length - 1]) { last = `${new URL(url).host}: not on this node`; continue; }
        if (body.params && body.params.request_type === "view_code" && j && j.result && String(j.result.code_base64 || "").length > MAX_CODE_B64) {
          return { status: 413, json: { jsonrpc: "2.0", id: body.id, error: { message: "refused by relay: contract code over 1 MB" } } };
        }
        remember(body, j);
        return { status: 200, json: j };
      }
      last = `${new URL(url).host}: HTTP ${r.status}`;
    } catch (e) { last = `${new URL(url).host}: ${short(e && e.message ? e.message : e)}`; }
  }
  return { status: 502, json: { jsonrpc: "2.0", id: body.id, error: { message: `testnet RPC unreachable (${last})` } } };
}
