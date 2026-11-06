// J1-J4: the path for a judge's or reviewer's agent. check_claims re-runs the
// README's claims in a private sandbox, the judge_tour prompt is served, and
// the repo's .mcp.json starts the servers a coding agent picks up on its own.
import { test } from "node:test";
import assert from "node:assert/strict";
import { existsSync, readFileSync } from "node:fs";
import { join } from "node:path";
import { connect } from "../src/client.js";
import { REPO, valid, writer } from "./helpers.js";

test("J1 check_claims: K1-K6 pass, schema-valid, every claim names its calls", async () => {
  const s = await connect({});
  try {
    const r = await s.call("check_claims", {});
    assert.ok(valid("check_claims", r), JSON.stringify(r.body));
    assert.equal(r.ok, true);
    assert.deepEqual(r.body.claims.map((c: { id: string }) => c.id), ["K1", "K2", "K3", "K4", "K5", "K6"]);
    for (const c of r.body.claims) assert.equal(c.pass, true, `${c.id}: ${c.observed}`);
    assert.equal(r.body.passed, 6);
    assert.equal(r.body.failed, 0);
    assert.equal(r.body.pass, true);
    const k4 = r.body.claims.find((c: { id: string }) => c.id === "K4");
    assert.match(k4.observed, /paid_now 690\.00, fee 10\.00, escrow 300\.00/);
  } finally {
    await s.close();
  }
});

test("J2 check_claims runs in its own sandbox: the server's state and spend are unchanged", async () => {
  const s = await writer();
  try {
    const before = await s.call("list_agents", {});
    const r = await s.call("check_claims", {});
    assert.equal(r.body.pass, true);
    const after = await s.call("list_agents", {});
    assert.deepEqual(after.body, before.body);
    // The real buy still works afterwards, inside the server's own cap.
    const buy = await s.call("buy_agent", { agent: "translator.testnet", max_price: 1000, confirm: true });
    assert.equal(buy.ok, true, JSON.stringify(buy.body));
    assert.equal(buy.body.session_spent, "1000.00");
  } finally {
    await s.close();
  }
});

test("J3 the judge_tour prompt is listed and names check_claims", async () => {
  const s = await connect({});
  try {
    const list = await s.client.listPrompts();
    assert.deepEqual(list.prompts.map((p) => p.name), ["judge_tour"]);
    const p = await s.client.getPrompt({ name: "judge_tour" });
    const text = (p.messages[0].content as { text: string }).text;
    assert.match(text, /check_claims/);
    assert.match(text, /verify_lockout/);
    assert.match(text, /xUSD-FAKE/);
  } finally {
    await s.close();
  }
});

test("J4 .mcp.json: both servers point at the built server, never mainnet; the offline one starts and checks its claims", async () => {
  const cfg = JSON.parse(readFileSync(join(REPO, ".mcp.json"), "utf8"));
  assert.deepEqual(Object.keys(cfg.mcpServers), ["agent-exit", "agent-exit-testnet"]);
  for (const s of Object.values(cfg.mcpServers) as { command: string; args: string[]; env: Record<string, string> }[]) {
    assert.equal(s.command, "node");
    assert.ok(existsSync(join(REPO, s.args[0])), s.args[0]);
    assert.ok(["offline", "testnet"].includes(s.env.AGENT_EXIT_NETWORK));
    assert.doesNotMatch(JSON.stringify(s), /mainnet|PRIVATE_KEY/i);
  }
  const t = cfg.mcpServers["agent-exit-testnet"].env;
  assert.equal(t.AGENT_EXIT_ALLOW_WRITE, undefined, "the testnet server is read-only");
  const s = await connect(cfg.mcpServers["agent-exit"].env);
  try {
    const r = await s.call("check_claims", {});
    assert.equal(r.body.pass, true);
  } finally {
    await s.close();
  }
});
