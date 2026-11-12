// S14: an MCP client (the summarizer agent) calls list_agents -> get_earnings
// -> scan_agent -> buy_agent{max_price:1000, confirm:true} -> verify_lockout;
// every response validates against schemas/mcp/*.v1.json; verify_lockout
// returns locked_out: true for alice.
import { test } from "node:test";
import assert from "node:assert/strict";
import { RUNNER_PK, valid, writer } from "./helpers.js";

test("S14 agent buys agent through MCP (offline, recorded contract results)", async () => {
  const s = await writer();
  try {
    const list = await s.call("list_agents");
    assert.ok(valid("list_agents", list));
    const t = list.body.agents.find((a: { agent: string }) => a.agent === "translator.factory.testnet");
    assert.deepEqual(
      { price: t.price, counted: t.counted, verified: t.verified, flagged: t.flagged, owner: t.owner },
      { price: "1000.00", counted: "360.00", verified: "120.00", flagged: "140.00", owner: "alice.testnet" },
    );

    const e = await s.call("get_earnings", { agent: "translator.factory.testnet" });
    assert.ok(valid("get_earnings", e));
    assert.equal(e.body.earnings.gross, "500.00");
    assert.equal(e.body.earnings.counted_payers, 6);

    const scan = await s.call("scan_agent", { agent: "translator.factory.testnet" });
    assert.ok(valid("scan_agent", scan));
    assert.equal(scan.body.verdict, "clean");
    assert.equal(scan.body.safe_to_buy, true);

    const buy = await s.call("buy_agent", {
      agent: "translator.factory.testnet", max_price: 1000, confirm: true,
    });
    assert.ok(valid("buy_agent", buy));
    assert.equal(buy.ok, true, JSON.stringify(buy.body));
    assert.equal(buy.body.owner_after, "summarizer.testnet");
    assert.deepEqual([buy.body.paid_now, buy.body.escrow], ["690.00", "300.00"]);
    assert.deepEqual(buy.body.key_batch.map((k: { action: string }) => k.action), ["delete", "add_fc"]);
    assert.equal(buy.body.key_batch[1].key, RUNNER_PK);
    assert.equal(buy.body.listing_version, scan.body.buy_args.listing_version);
    assert.equal(buy.body.known_keys_hash, scan.body.buy_args.known_keys_hash);

    const lock = await s.call("verify_lockout", { agent: "translator.factory.testnet", account: "alice.testnet" });
    assert.ok(valid("verify_lockout", lock));
    assert.equal(lock.body.locked_out, true, JSON.stringify(lock.body.checks));
    assert.ok(lock.body.checks.some((c: { id: string }) => c.id === "refused_withdraw"));

    const me = await s.call("verify_lockout", { agent: "translator.factory.testnet", account: "summarizer.testnet" });
    assert.equal(me.body.locked_out, false, "the new owner is not locked out");

    // A second buy of the same agent: it is no longer listed.
    const again = await s.call("buy_agent", {
      agent: "translator.factory.testnet", max_price: 1000, confirm: true,
    });
    assert.equal(again.body.error.code, "NOT_LISTED");
  } finally {
    await s.close();
  }
});
