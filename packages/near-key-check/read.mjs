// Read an account's facts from a NEAR testnet RPC (view calls only: nothing
// is ever signed or sent). Same reads as the demo page (web/account.js).
import { wasmExports, EMPTY_CODE } from "./account-check.mjs";

export const TESTNET_RPC = "https://rpc.testnet.fastnear.com";
export const ACCOUNT = /^([a-z0-9_-]+(\.[a-z0-9_-]+)*\.testnet|[0-9a-f]{64})$/;

export async function readFacts(account, { rpc = TESTNET_RPC, fetchImpl = fetch } = {}) {
  if (!ACCOUNT.test(account) || account.length > 64) throw new Error("not a NEAR testnet account (name.testnet or 64-hex implicit)");
  if (/mainnet|\.near\.org\/?$/.test(rpc) && !/testnet/.test(rpc)) throw new Error("testnet RPC only");
  const query = async (params) => {
    const r = await fetchImpl(rpc, { method: "POST", headers: { "content-type": "application/json" },
      body: JSON.stringify({ jsonrpc: "2.0", id: "check", method: "query", params: { finality: "final", ...params } }) });
    const j = await r.json();
    if (j.error) throw new Error(String(j.error.cause?.name || j.error.message || "RPC error").slice(0, 160));
    if (j.result && j.result.error) throw new Error(String(j.result.error).slice(0, 160));
    return j.result;
  };
  const acc = await query({ request_type: "view_account", account_id: account });
  const list = await query({ request_type: "view_access_key_list", account_id: account });
  const perm = (p) => (p === "FullAccess" ? "FullAccess" : p && p.FunctionCall ? p.FunctionCall : null);
  const keys = (list.keys || []).map((k) => ({ public_key: k.public_key, permission: perm(k.access_key && k.access_key.permission) }));
  let methods = [];
  if (acc.code_hash !== EMPTY_CODE && !acc.global_contract_account_id) {
    try {
      const c = await query({ request_type: "view_code", account_id: account });
      methods = wasmExports(Uint8Array.from(Buffer.from(c.code_base64, "base64")));
    } catch {
      methods = null;
    }
  }
  let known_keys = null;
  try {
    const r = await query({ request_type: "call_function", account_id: account, method_name: "get_status", args_base64: Buffer.from("{}").toString("base64") });
    const s = JSON.parse(Buffer.from(r.result).toString("utf8"));
    if (Array.isArray(s.known_keys)) known_keys = s.known_keys.map(String);
  } catch {
    known_keys = null;
  }
  return { account, code_hash: acc.code_hash, methods, keys, known_keys,
    global_contract_account_id: acc.global_contract_account_id || null, global_contract_hash: acc.global_contract_hash || null };
}
