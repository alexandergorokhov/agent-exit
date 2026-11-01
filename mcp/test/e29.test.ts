// E29: buy_agent without confirm, above max_price, or with any scan finding:
// refused before any transaction, JSON error with the reason.
import { test } from "node:test";
import assert from "node:assert/strict";
import { RUNNER_PK, valid, writer } from "./helpers.js";

const buy = (agent: string, extra: Record<string, unknown>) => ({
  agent, max_price: 1000, confirm: true, buyer_runner_pk: RUNNER_PK, ...extra,
});

test("E29 refusals happen before anything is sent", async () => {
  const s = await writer();
  try {
    const cases: [Record<string, unknown>, string][] = [
      [buy("translator.testnet", { confirm: false }), "CONFIRM_REQUIRED"],
      [buy("translator.testnet", { max_price: 999.99 }), "OVER_MAX_PRICE"],
      [buy("cheat.testnet", { max_price: 600 }), "SCAN_BLOCKED"],
      [{ agent: "translator.testnet", confirm: true, buyer_runner_pk: RUNNER_PK }, "BAD_INPUT"], // no max_price
      [buy("translator.testnet", { buyer_runner_pk: "ed25519:not-a-key" }), "BAD_INPUT"],
      [buy("nobody.testnet", {}), "UNKNOWN_AGENT"],
    ];
    for (const [args, code] of cases) {
      const r = await s.call("buy_agent", args);
      assert.equal(r.ok, false, JSON.stringify(args));
      assert.ok(valid("buy_agent", r));
      assert.equal(r.body.error.code, code, JSON.stringify(r.body));
      assert.equal(r.body.sent, false);
    }
    const blocked = await s.call("buy_agent", buy("cheat.testnet", { max_price: 600 }));
    assert.deepEqual(
      blocked.body.error.details.findings.map((f: { code: string }) => f.code),
      ["full_access_key_present"],
    );
    // Nothing changed: translator is still alice's and still listed.
    const list = await s.call("list_agents");
    const t = list.body.agents.find((a: { agent: string }) => a.agent === "translator.testnet");
    assert.deepEqual([t.owner, t.status], ["alice.testnet", "Listed"]);
  } finally {
    await s.close();
  }
});

test("E29 session spend cap", async () => {
  const s = await writer("summarizer.testnet", { AGENT_EXIT_SESSION_CAP: "999" });
  try {
    const r = await s.call("buy_agent", buy("translator.testnet", {}));
    assert.equal(r.body.error.code, "SESSION_CAP");
  } finally {
    await s.close();
  }
});

test("E29 an unrecorded offline buy is refused, not faked", async () => {
  const s = await writer("carol.testnet");
  try {
    const r = await s.call("buy_agent", buy("translator.testnet", {}));
    assert.equal(r.body.error.code, "OFFLINE_NO_RECORDING");
  } finally {
    await s.close();
  }
});
