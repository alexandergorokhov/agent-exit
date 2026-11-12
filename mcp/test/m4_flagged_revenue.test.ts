// Review 6, M4: buy_agent refuses an agent whose revenue is mostly flagged
// (self-paid, sub-account, round trip) unless the call accepts it; any
// flagged revenue on a bought agent is returned as a warning.
import { test } from "node:test";
import assert from "node:assert/strict";
import { join } from "node:path";
import { OfflineBackend, type AgentView } from "../src/backend.js";
import { loadConfig } from "../src/config.js";
import { loadSchemas, Tools } from "../src/tools.js";
import { REPO, RUNNER_PK } from "./helpers.js";

const WORLD = join(REPO, "fixtures", "mcp", "world.json");
const env = { AGENT_EXIT_ALLOW_WRITE: "1", AGENT_EXIT_ACCOUNT_ID: "summarizer.testnet", AGENT_EXIT_RUNNER_PK: RUNNER_PK };
const schemas = loadSchemas(REPO);

/** translator.factory.testnet with pump-bot's meter: gross 410, flagged 400, counted 10. */
class PumpedWorld extends OfflineBackend {
  override async agent(id: string): Promise<AgentView> {
    const v = await super.agent(id);
    if (id !== "translator.factory.testnet") return v;
    return { ...v, earnings: { ...v.earnings, gross: "410000000", flagged: "400000000", flagged_self: "400000000", counted: "10000000", verified: "0" } };
  }
}

test("M4a mostly-flagged revenue: REVENUE_MOSTLY_FLAGGED, sent false, reservation released", async () => {
  const price = Number((await new OfflineBackend(WORLD).agent("translator.factory.testnet")).listing.price) / 1e6;
  // The cap fits exactly one buy: a leaked reservation would make the second call fail.
  const t = new Tools(loadConfig({ ...env, AGENT_EXIT_SESSION_CAP: String(price) }), new PumpedWorld(WORLD), schemas);
  const r = await t.call("buy_agent", { agent: "translator.factory.testnet", max_price: 1000, confirm: true });
  assert.equal(r.ok, false);
  assert.equal(r.body.error.code, "REVENUE_MOSTLY_FLAGGED", JSON.stringify(r.body));
  assert.equal(r.body.sent, false);
  assert.match(r.body.error.message, /400\.00 of 410\.00/);
  assert.match(r.body.error.message, /accept_flagged_revenue/);
  assert.equal(schemas.validateError(r.body), true);

  const ok = await t.call("buy_agent", { agent: "translator.factory.testnet", max_price: 1000, confirm: true, accept_flagged_revenue: true });
  assert.equal(ok.ok, true, JSON.stringify(ok.body));
  assert.equal(ok.body.owner_after, "summarizer.testnet");
  const w = ok.body.warnings.find((x: any) => x.code === "revenue_flagged");
  assert.ok(w, JSON.stringify(ok.body.warnings));
  assert.equal(w.severity, "warn");
  assert.equal(schemas.validateOutput.buy_agent(ok.body), true, JSON.stringify(schemas.validateOutput.buy_agent.errors));
});

test("M4b some flagged revenue (under half): bought, with a revenue_flagged warning", async () => {
  const t = new Tools(loadConfig(env), new OfflineBackend(WORLD), schemas);
  const r = await t.call("buy_agent", { agent: "translator.factory.testnet", max_price: 1000, confirm: true });
  assert.equal(r.ok, true, JSON.stringify(r.body));
  const w = r.body.warnings.find((x: any) => x.code === "revenue_flagged");
  assert.ok(w, JSON.stringify(r.body.warnings));
  assert.match(w.untrusted_detail, /140\.00 of 500\.00/);
});

test("M4c accept_flagged_revenue is in the buy_agent input schema", () => {
  assert.equal(schemas.validateInput.buy_agent({ agent: "translator.factory.testnet", max_price: 1, confirm: true, accept_flagged_revenue: true }), true);
});
