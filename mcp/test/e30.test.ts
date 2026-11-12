// E30: server started without a key/account in env: read tools work, write
// tools return WRITE_DISABLED.
import { test } from "node:test";
import assert from "node:assert/strict";
import { connect } from "../src/client.js";
import { valid } from "./helpers.js";

const envs: Record<string, string>[] = [{}, { AGENT_EXIT_ALLOW_WRITE: "1" }, { AGENT_EXIT_ACCOUNT_ID: "summarizer.testnet" }];
for (const env of envs) {
  test(`E30 writes disabled with env ${JSON.stringify(env)}`, async () => {
    const s = await connect(env);
    try {
      for (const [tool, args] of [
        ["list_agents", {}],
        ["get_earnings", { agent: "translator.factory.testnet" }],
        ["scan_agent", { agent: "translator.factory.testnet" }],
        ["verify_lockout", { agent: "translator.factory.testnet", account: "alice.testnet" }],
      ] as const) {
        const r = await s.call(tool, args);
        assert.equal(r.ok, true, `${tool}: ${JSON.stringify(r.body)}`);
        assert.ok(valid(tool, r));
      }
      const b = await s.call("buy_agent", {
        agent: "translator.factory.testnet", max_price: 1000, confirm: true,
      });
      assert.equal(b.body.error.code, "WRITE_DISABLED");
      // A blocked agent is still reported as blocked: the scan is a read.
      const c = await s.call("buy_agent", { agent: "cheat.testnet", max_price: 600, confirm: true });
      assert.equal(c.body.error.code, "SCAN_BLOCKED", JSON.stringify(c.body));
      assert.equal(c.body.sent, false);
      const l = await s.call("list_for_sale", { agent: "translator.factory.testnet", price: 1200, confirm: true });
      assert.equal(l.body.error.code, "WRITE_DISABLED");
      const sh = await s.call("buy_share", { agent: "research.factory.testnet", offer_id: 1, pct: 10, max_price: 150, confirm: true });
      assert.equal(sh.body.error.code, "WRITE_DISABLED");
      const tools = await s.client.listTools();
      const ro = Object.fromEntries(tools.tools.map((t) => [t.name, t.annotations?.readOnlyHint]));
      assert.deepEqual(ro, {
        list_agents: true, get_earnings: true, scan_agent: true,
        buy_agent: false, verify_lockout: true, list_for_sale: false,
        list_offers: true, buy_share: false, portfolio: true, check_claims: true, sandbox: true, check_account: true, sealed_delivery: false,
      });
      // check_claims buys only inside its own sandbox, so it works here too.
      const k = await s.call("check_claims", {});
      assert.equal(k.body.pass, true, JSON.stringify(k.body));
    } finally {
      await s.close();
    }
  });
}
