// K-series for near-key-check: the package gives the same verdicts as the
// demo page and the MCP tool, reads only (view calls), and its guard refuses
// any account that is not locked.
import { test } from "node:test";
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { spawnSync } from "node:child_process";
import { fileURLToPath } from "node:url";
import { checkAccount, assertNoBackdoor, assess, readFacts } from "../index.mjs";

const here = (p) => fileURLToPath(new URL(p, import.meta.url));
const accounts = JSON.parse(readFileSync(here("../../../fixtures/mcp/accounts.json"), "utf8")).accounts;

/** A minimal wasm module that exports `names` as functions. */
function wasm(names) {
  const leb = (n) => { const o = []; do { let b = n & 0x7f; n >>>= 7; if (n) b |= 0x80; o.push(b); } while (n); return o; };
  const body = [...leb(names.length), ...names.flatMap((n) => [...leb(n.length), ...Buffer.from(n), 0, 0])];
  return Buffer.from([0, 0x61, 0x73, 0x6d, 1, 0, 0, 0, 7, ...leb(body.length), ...body]);
}

/** A fake RPC: serves one fixture account; records every request type. */
function rpc(id, f, known = null) {
  const seen = [];
  const fetchImpl = async (_url, init) => {
    const { method, params } = JSON.parse(init.body);
    seen.push(params.request_type);
    assert.equal(method, "query", "view queries only");
    const ok = (result) => ({ json: async () => ({ result }) });
    if (params.request_type === "view_account") return ok({ code_hash: f.code_hash, global_contract_account_id: f.global_contract_account_id });
    if (params.request_type === "view_access_key_list") return ok({ keys: f.keys.map((k) => ({ public_key: k.public_key, access_key: { permission: k.permission && typeof k.permission === "object" ? { FunctionCall: k.permission } : k.permission } })) });
    if (params.request_type === "view_code") return ok({ code_base64: wasm(f.methods).toString("base64") });
    if (params.request_type === "call_function") return known ? ok({ result: [...Buffer.from(JSON.stringify({ known_keys: known }))] }) : { json: async () => ({ error: { message: "MethodNotFound" } }) };
    throw new Error(`unexpected ${params.request_type}`);
  };
  return { fetchImpl, seen, id };
}

test("NK1 the package's rules are the page's rules, byte for byte", () => {
  assert.equal(readFileSync(here("../account-check.mjs"), "utf8"), readFileSync(here("../../../web/account-check.mjs"), "utf8"));
});

test("NK2 every fixture account: reading it and assessing gives the same verdict as the page on the same facts", async () => {
  for (const [id, f] of Object.entries(accounts)) {
    const r = rpc(id, f);
    const got = await checkAccount(id, { fetchImpl: r.fetchImpl, rpc: "https://rpc.testnet.fastnear.com" });
    const want = assess({ account: id, known_keys: null, ...f });
    assert.equal(got.verdict, want.verdict, id);
    assert.ok(r.seen.every((t) => ["view_account", "view_access_key_list", "view_code", "call_function"].includes(t)), "read-only");
  }
});

test("NK3 the guard refuses a backdoored bot and passes the published build with only its own keys", async () => {
  const bot = rpc("trading-bot.testnet", accounts["trading-bot.testnet"]);
  await assert.rejects(assertNoBackdoor("trading-bot.testnet", { fetchImpl: bot.fetchImpl }), /backdoor_risk/);
  const runner = { public_key: "ed25519:runner", permission: { receiver_id: "agent.testnet", method_names: ["heartbeat"], allowance: "5000" } };
  const agent = { code_hash: "PUBLISHED", methods: ["new", "heartbeat", "pay"], keys: [runner] };
  const a = rpc("agent.testnet", agent, ["ed25519:runner"]);
  const ok = await assertNoBackdoor("agent.testnet", { fetchImpl: a.fetchImpl, publishedHash: "PUBLISHED" });
  assert.equal(ok.verdict, "locked");
  // A key the contract never installed: not locked.
  const b = rpc("agent.testnet", { ...agent, keys: [runner, { ...runner, public_key: "ed25519:stray" }] }, ["ed25519:runner"]);
  await assert.rejects(assertNoBackdoor("agent.testnet", { fetchImpl: b.fetchImpl, publishedHash: "PUBLISHED" }), /key_not_issued_by_contract|backdoor_risk/);
});

test("NK4 only testnet accounts and RPCs; the CLI exits 2 on bad input", async () => {
  await assert.rejects(readFacts("alice.near"), /testnet/);
  await assert.rejects(readFacts("a.testnet", { rpc: "https://rpc.mainnet.near.org" }), /testnet RPC only/);
  const r = spawnSync(process.execPath, [here("../cli.mjs")], { encoding: "utf8" });
  assert.equal(r.status, 2);
  assert.match(r.stderr, /usage/);
});
