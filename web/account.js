// "Check any NEAR account" box: reads the account's keys, code and (for the
// published build) the contract's own key list through the page's read-only
// relay (api/rpc, web/rpc-allow.mjs), then runs web/account-check.mjs.
import { assess, wasmExports, EMPTY_CODE } from "./account-check.mjs";

const ACCOUNT = /^([a-z0-9_-]+(\.[a-z0-9_-]+)*\.testnet|[0-9a-f]{64})$/;

async function query(params) {
  const r = await fetch("api/rpc", { method: "POST", headers: { "content-type": "application/json" },
    body: JSON.stringify({ jsonrpc: "2.0", id: "check", method: "query", params: { finality: "final", ...params } }) });
  const j = await r.json();
  if (j.error) throw new Error(j.error.cause?.name || j.error.message || "RPC error");
  if (j.result && j.result.error) throw new Error(String(j.result.error).slice(0, 160));
  return j.result;
}

const perm = (p) => p === "FullAccess" ? "FullAccess" : p && p.FunctionCall ? p.FunctionCall : null;

export async function readFacts(account) {
  const acc = await query({ request_type: "view_account", account_id: account });
  const list = await query({ request_type: "view_access_key_list", account_id: account });
  const keys = (list.keys || []).map((k) => ({ public_key: k.public_key, permission: perm(k.access_key && k.access_key.permission) }));
  let methods = [];
  if (acc.code_hash !== EMPTY_CODE && !acc.global_contract_account_id) {
    try {
      const c = await query({ request_type: "view_code", account_id: account });
      methods = wasmExports(Uint8Array.from(atob(c.code_base64), (ch) => ch.charCodeAt(0)));
    } catch {
      methods = null;
    }
  }
  return { account, code_hash: acc.code_hash, methods, keys, global_contract_account_id: acc.global_contract_account_id || null, global_contract_hash: acc.global_contract_hash || null };
}

async function knownKeys(account) {
  try {
    const r = await query({ request_type: "call_function", account_id: account, method_name: "get_status", args_base64: btoa("{}") });
    const s = JSON.parse(new TextDecoder().decode(new Uint8Array(r.result)));
    return Array.isArray(s.known_keys) ? s.known_keys.map(String) : null;
  } catch {
    return null;
  }
}

const el = (tag, attrs = {}, ...kids) => {
  const e = document.createElement(tag);
  for (const [k, v] of Object.entries(attrs)) if (v !== null && v !== undefined) k === "text" ? (e.textContent = v) : e.setAttribute(k, v);
  for (const c of kids) if (c) e.append(c);
  return e;
};

async function published() {
  try {
    return (await (await fetch("data/testnet.json")).json()).agent_code_hash;
  } catch {
    return null;
  }
}

function init() {
  const form = document.getElementById("check-form");
  if (!form) return;
  const out = document.getElementById("check-out");
  form.addEventListener("submit", async (ev) => {
    ev.preventDefault();
    const id = String(new FormData(form).get("account") || "").trim().toLowerCase();
    if (!ACCOUNT.test(id) || id.length > 64) {
      out.replaceChildren(el("p", { class: "err", text: "Enter a NEAR testnet account (name.testnet) or a 64-hex implicit account." }));
      return;
    }
    out.replaceChildren(el("p", { class: "muted", text: `Reading ${id} from NEAR testnet…` }));
    try {
      const facts = await readFacts(id);
      const publishedHash = await published();
      if (publishedHash && facts.code_hash === publishedHash) facts.known_keys = await knownKeys(id);
      const r = assess(facts, { publishedHash });
      const cls = r.verdict === "locked" ? "ok" : r.verdict === "backdoor_risk" ? "error" : "warn";
      out.replaceChildren(...[
        el("p", {}, el("span", { class: `badge ${cls}`, text: r.verdict }), " ", r.summary),
        r.findings.length ? el("ul", { class: "find" }, ...r.findings.map((f) => el("li", {}, el("code", { text: f.code }), ` (${f.severity}) ${f.text}`))) : null,
        el("details", {}, el("summary", { text: `${r.keys.length} access key(s)` }), el("ul", { class: "find" }, ...r.keys.map((k) => el("li", {}, el("code", { text: k.public_key }), `: ${k.can}`)))),
        el("p", { class: "muted", text: `Read-only, through this page's relay; code hash ${facts.code_hash}. The same rules as the MCP tool check_account and make check-account.` })].filter(Boolean));
    } catch (e) {
      out.replaceChildren(el("p", { class: "err", text: `Could not read ${id}: ${e && e.message ? e.message : e}` }));
    }
  });
}

if (typeof document !== "undefined") init();
