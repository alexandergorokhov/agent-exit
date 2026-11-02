// S15 (MCP part): the new owner lists the agent again; the old seller cannot.
import { test } from "node:test";
import assert from "node:assert/strict";
import { valid, writer } from "./helpers.js";

test("S15 resale through MCP", async () => {
  const alice = await writer("alice.testnet");
  try {
    // Before the sale alice owns translator, but it is already listed and
    // this price was not recorded: nothing is faked.
    const r = await alice.call("list_for_sale", { agent: "translator.testnet", price: 1200, confirm: true });
    assert.equal(r.body.error.code, "OFFLINE_NO_RECORDING");
  } finally {
    await alice.close();
  }
  const s = await writer();
  try {
    const nc = await s.call("list_for_sale", { agent: "translator.testnet", price: 1200, confirm: false });
    assert.equal(nc.body.error.code, "CONFIRM_REQUIRED");
    const early = await s.call("list_for_sale", { agent: "translator.testnet", price: 1200, confirm: true });
    assert.equal(early.body.error.code, "NOT_OWNER");
    await s.call("buy_agent", { agent: "translator.testnet", max_price: 1000, confirm: true });
    const l = await s.call("list_for_sale", { agent: "translator.testnet", price: 1200, confirm: true });
    assert.ok(valid("list_for_sale", l));
    assert.equal(l.ok, true, JSON.stringify(l.body));
    assert.deepEqual([l.body.seller, l.body.price, l.body.paid_now, l.body.escrow], ["summarizer.testnet", "1200.00", "840.00", "360.00"]);
    assert.equal(l.body.listing_version, 3);
    const list = await s.call("list_agents", { status: "Listed" });
    const t = list.body.agents.find((a: { agent: string }) => a.agent === "translator.testnet");
    assert.deepEqual([t.owner, t.price, t.counted], ["summarizer.testnet", "1200.00", "360.00"], "history travels");
  } finally {
    await s.close();
  }
});
