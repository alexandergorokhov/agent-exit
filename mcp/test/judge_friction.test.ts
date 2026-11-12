// JF1-JF3 (judge friction): verify_lockout defaults `account` to the recorded
// seller, sandbox {action: "help"} lists every action with its args and
// example sequences from the code's own table, and an unknown agent names the
// agents this server knows.
import { test } from "node:test";
import assert from "node:assert/strict";
import { join } from "node:path";
import { readFileSync } from "node:fs";
import { OfflineBackend, TestnetBackend } from "../src/backend.js";
import { loadConfig } from "../src/config.js";
import { connect } from "../src/client.js";
import { SANDBOX_ACTIONS, SANDBOX_ACTION_TABLE, SANDBOX_EXAMPLES } from "../src/sandbox.js";
import { loadSchemas, Tools } from "../src/tools.js";
import { REPO, RUNNER_PK, valid } from "./helpers.js";

const WORLD = join(REPO, "fixtures", "mcp", "world.json");
const env = { AGENT_EXIT_ALLOW_WRITE: "1", AGENT_EXIT_ACCOUNT_ID: "summarizer.testnet", AGENT_EXIT_RUNNER_PK: RUNNER_PK };
const offline = () => new Tools(loadConfig(env), new OfflineBackend(WORLD), loadSchemas(REPO));

// ------------------------------------------------------------------ JF1

test("JF1 verify_lockout without account checks the recorded seller of the last sale", async () => {
  const t = offline();
  const buy = await t.call("buy_agent", { agent: "translator.testnet", max_price: 1000, confirm: true });
  assert.equal(buy.ok, true, JSON.stringify(buy.body));
  const r = await t.call("verify_lockout", { agent: "translator.testnet" });
  assert.ok(valid("verify_lockout", r), JSON.stringify(r.body));
  assert.equal(r.ok, true, JSON.stringify(r.body));
  assert.equal(r.body.account, "alice.testnet");
  assert.equal(r.body.account_checked, "alice.testnet");
  assert.match(r.body.account_source, /^default: recorded seller/);
  assert.match(r.body.account_source, /sale 1/);
  assert.equal(r.body.locked_out, true);
  // Same answer as naming the account.
  const named = await t.call("verify_lockout", { agent: "translator.testnet", account: "alice.testnet" });
  assert.equal(named.body.locked_out, true);
  assert.deepEqual(named.body.checks, r.body.checks);
});

test("JF1 an explicit account keeps the old output (no default fields)", async () => {
  const r = await offline().call("verify_lockout", { agent: "translator.testnet", account: "alice.testnet" });
  assert.equal(r.ok, true);
  assert.equal(r.body.account_checked, undefined);
  assert.equal(r.body.account_source, undefined);
});

test("JF1 before any sale it falls back to the listing's seller and says so", async () => {
  const r = await offline().call("verify_lockout", { agent: "translator.testnet" });
  assert.ok(valid("verify_lockout", r), JSON.stringify(r.body));
  assert.equal(r.body.account_checked, "alice.testnet");
  assert.match(r.body.account_source, /^default: recorded seller \(current listing/);
  assert.equal(r.body.locked_out, false, "alice still owns it before the sale");
});

test("JF1 no sale and no listing: BAD_INPUT asking for account", async () => {
  const r = await offline().call("verify_lockout", { agent: "research.testnet" });
  assert.ok(valid("verify_lockout", r));
  assert.equal(r.ok, false);
  assert.equal(r.body.error.code, "BAD_INPUT");
  assert.match(r.body.error.message, /no recorded seller.*pass account/);
});

test("JF1 the input schema no longer requires account", () => {
  const s = JSON.parse(readFileSync(join(REPO, "schemas", "mcp", "verify_lockout.input.v1.json"), "utf8"));
  assert.deepEqual(s.required, ["agent"]);
});

// ------------------------------------------------------------------ JF2

test("JF2 sandbox help lists every action with its args and three example sequences", async () => {
  const s = await connect({});
  try {
    const r = await s.call("sandbox", { action: "help" });
    assert.ok(valid("sandbox", r), JSON.stringify(r.body));
    const h = r.body.help;
    assert.deepEqual(h.actions.map((a: { action: string }) => a.action), [...SANDBOX_ACTIONS]);
    const list = h.actions.find((a: { action: string }) => a.action === "list");
    assert.deepEqual(Object.keys(list.args).sort(), ["pay_from_revenue", "price"]);
    assert.deepEqual(Object.keys(h.actions.find((a: { action: string }) => a.action === "buy").args), ["skip_scan"]);
    assert.deepEqual(Object.keys(h.actions.find((a: { action: string }) => a.action === "wait").args), ["days"]);
    assert.deepEqual(Object.keys(h.examples).sort(), ["cheat", "honest_sale", "invest"]);
    assert.deepEqual(h.examples.invest.steps.map((x: { action: string }) => x.action), ["reset", "offer_share", "buy_share", "pay_customer", "claim_share"]);
    assert.equal(r.body.step, 0, "help runs nothing");
  } finally {
    await s.close();
  }
});

test("JF2 the schema enum is the code's action table plus help", () => {
  const s = JSON.parse(readFileSync(join(REPO, "schemas", "mcp", "sandbox.input.v1.json"), "utf8"));
  assert.deepEqual(s.properties.action.enum, ["help", ...SANDBOX_ACTIONS]);
  assert.deepEqual([...SANDBOX_ACTIONS], Object.keys(SANDBOX_ACTION_TABLE));
  // Every arg named in the table is an input property.
  for (const row of Object.values(SANDBOX_ACTION_TABLE)) for (const a of Object.keys(row.args)) assert.ok(s.properties[a], a);
});

test("JF2 every example sequence runs as described in the real wasm", async () => {
  for (const [name, ex] of Object.entries(SANDBOX_EXAMPLES)) {
    const s = await connect({});
    try {
      let last: any;
      for (const step of ex.steps) {
        const r = await s.call("sandbox", step);
        assert.equal(r.ok, true, `${name} ${JSON.stringify(step)}: ${JSON.stringify(r.body)}`);
        last = r.body;
      }
      if (name === "invest") assert.match(last.result, /5\.00 sent to dave/);
      if (name === "honest_sale") assert.equal(last.state.owner, "bob.testnet");
      if (name === "cheat") {
        assert.equal(last.ok, false);
        assert.match(last.result, /SCAN_BLOCKED/);
      }
    } finally {
      await s.close();
    }
  }
});

// ------------------------------------------------------------------ JF3

test("JF3 offline UNKNOWN_AGENT lists the world's agents", async () => {
  const t = offline();
  for (const [tool, args] of [
    ["get_earnings", { agent: "nobody.testnet" }],
    ["scan_agent", { agent: "nobody.testnet" }],
    ["verify_lockout", { agent: "nobody.testnet", account: "alice.testnet" }],
    ["verify_lockout", { agent: "nobody.testnet" }],
  ] as const) {
    const r = await t.call(tool, args);
    assert.equal(r.ok, false);
    assert.equal(r.body.error.code, "UNKNOWN_AGENT", tool);
    assert.match(r.body.error.message, /known agents: cheat\.testnet, research\.testnet, translator\.testnet$/, tool);
  }
});

test("JF3 a sealed-delivery unknown agent lists the sealed fixtures' agents once", async () => {
  const r = await offline().call("sealed_delivery", { agent: "translator.testnet", sale_id: 1, action: "status" });
  assert.equal(r.body.error.code, "UNKNOWN_AGENT");
  assert.match(r.body.error.message, /known agents: brainy\.testnet, swapped\.testnet$/);
  assert.equal(r.body.error.message.match(/known agents/g).length, 1);
});

test("JF3 testnet: an account that does not exist is UNKNOWN_AGENT naming AGENT_EXIT_AGENTS", async () => {
  const f = (async () =>
    new Response(JSON.stringify({ error: { name: "HANDLER_ERROR", cause: { name: "UNKNOWN_ACCOUNT" }, message: "account nobody.testnet does not exist while viewing" } }), {
      status: 200,
    })) as unknown as typeof fetch;
  const cfg = loadConfig({ AGENT_EXIT_NETWORK: "testnet", AGENT_EXIT_AGENTS: "translator.agentexit5.testnet,cheat.agentexit5.testnet" });
  const t = new Tools(cfg, new TestnetBackend(cfg, f), loadSchemas(REPO));
  const r = await t.call("get_earnings", { agent: "nobody.testnet" });
  assert.equal(r.ok, false);
  assert.equal(r.body.error.code, "UNKNOWN_AGENT", JSON.stringify(r.body));
  assert.match(r.body.error.message, /known agents: cheat\.agentexit5\.testnet, translator\.agentexit5\.testnet$/);
});

test("JF3 testnet allowlist refusals name the known agents too", async () => {
  const mk = (agents: string) =>
    new Tools(loadConfig({ ...env, AGENT_EXIT_NETWORK: "testnet", AGENT_EXIT_PRIVATE_KEY: "ed25519:x", AGENT_EXIT_AGENTS: agents }), new OfflineBackend(WORLD), loadSchemas(REPO));
  let r = await mk("translator.agentexit5.testnet").call("buy_agent", { agent: "other.testnet", max_price: 1, confirm: true });
  assert.equal(r.body.error.code, "UNKNOWN_AGENT");
  assert.match(r.body.error.message, /AGENT_EXIT_AGENTS/);
  assert.match(r.body.error.message, /known agents: translator\.agentexit5\.testnet$/);
  r = await mk("").call("buy_agent", { agent: "other.testnet", max_price: 1, confirm: true });
  assert.match(r.body.error.message, /known agents: none \(AGENT_EXIT_AGENTS is empty\)$/);
});
