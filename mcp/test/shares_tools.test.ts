// MCP share tools (docs/SHARES.md): list_offers, buy_share, portfolio.
// Offline, every result is RECORDED from the real contract code
// (fixtures/mcp/world.json: research.factory.testnet, carol offers 20% for a year
// at 300.00; the recorded buy is dave taking 10% for 150.00).
import { test } from "node:test";
import assert from "node:assert/strict";
import { connect } from "../src/client.js";
import { valid, writer } from "./helpers.js";

const BUY = { agent: "research.factory.testnet", offer_id: 1, pct: 10, max_price: 150, confirm: true };

test("ST1 list_offers shows the open slice next to counted earnings", async () => {
  const s = await connect({});
  try {
    const r = await s.call("list_offers");
    assert.ok(valid("list_offers", r), JSON.stringify(r.body));
    assert.deepEqual(r.body.agents.map((a: { agent: string }) => a.agent), ["research.factory.testnet"]);
    const a = r.body.agents[0];
    assert.equal(a.owner, "carol.testnet");
    assert.equal(a.counted, "240.00");
    assert.equal(a.buyable, true);
    assert.deepEqual(a.offers[0], {
      offer_id: 1, seller: "carol.testnet", pct: 20, price: "300.00", price_per_pct: "15.00",
      sold_pct: 0, open_pct: 20, min_part_pct: 2, ends: "2027-01-11", holders: 0, seller_is_owner: true,
    });
    // One named agent with no offers is still answered.
    const t = await s.call("list_offers", { agent: "translator.factory.testnet" });
    assert.ok(valid("list_offers", t));
    assert.equal(t.body.agents[0].offers.length, 0);
    assert.equal(t.body.agents[0].buyable, false, "listed: share buys refused");
  } finally {
    await s.close();
  }
});

test("ST2 buy_share is off without writes, and needs confirm", async () => {
  const ro = await connect({});
  try {
    const r = await ro.call("buy_share", BUY);
    assert.ok(valid("buy_share", r));
    assert.equal(r.body.error.code, "WRITE_DISABLED");
  } finally {
    await ro.close();
  }
  const s = await writer("dave.testnet");
  try {
    const r = await s.call("buy_share", { ...BUY, confirm: false });
    assert.equal(r.body.error.code, "CONFIRM_REQUIRED");
  } finally {
    await s.close();
  }
});

test("ST3 refusals before anything is sent: price, size, offer, owner, listed agent", async () => {
  const s = await writer("dave.testnet");
  try {
    const code = async (a: object) => (await s.call("buy_share", { ...BUY, ...a })).body.error?.code;
    assert.equal(await code({ max_price: 149.99 }), "OVER_MAX_PRICE");
    assert.equal(await code({ pct: 1, max_price: 1000 }), "SHARE_TOO_SMALL");
    assert.equal(await code({ pct: 25, max_price: 1000 }), "OFFER_CLOSED");
    assert.equal(await code({ offer_id: 9 }), "UNKNOWN_OFFER");
    assert.equal(await code({ pct: 10.005 }), "BAD_INPUT");
    assert.equal(await code({ agent: "translator.factory.testnet" }), "NOT_BUYABLE");
  } finally {
    await s.close();
  }
  const carol = await writer("bot.carol.testnet");
  try {
    assert.equal((await carol.call("buy_share", BUY)).body.error.code, "NOT_BUYABLE");
  } finally {
    await carol.close();
  }
});

test("ST4 dave buys 10% after a clean investor scan; portfolio shows it", async () => {
  const s = await writer("dave.testnet");
  try {
    const before = await s.call("portfolio");
    assert.ok(valid("portfolio", before));
    assert.equal(before.body.holdings.length, 0);
    const r = await s.call("buy_share", BUY);
    assert.ok(valid("buy_share", r), JSON.stringify(r.body));
    assert.equal(r.ok, true, JSON.stringify(r.body));
    assert.equal(r.body.cost, "150.00");
    assert.equal(r.body.refund, "0.00");
    // The 1% protocol fee (1.50) comes out of the owner's upfront half.
    assert.equal(r.body.paid_to_owner_now, "73.50");
    assert.equal(r.body.protocol_fee, "1.50");
    assert.equal(r.body.held_in_escrow, "75.00");
    assert.equal(r.body.session_spent, "150.00");
    assert.equal(r.body.recorded, true);
    const p = await s.call("portfolio");
    assert.ok(valid("portfolio", p), JSON.stringify(p.body));
    assert.deepEqual(p.body.holdings, [
      { agent: "research.factory.testnet", offer_id: 1, pct: 10, paid_in: "150.00", earned: "0.00", escrow: "75.00", ends: "2027-01-11" },
    ]);
    assert.equal(p.body.total_paid_in, "150.00");
    const o = await s.call("list_offers", { agent: "research.factory.testnet" });
    assert.equal(o.body.agents[0].offers[0].open_pct, 10);
    assert.equal(o.body.agents[0].offers[0].sold_pct, 10);
  } finally {
    await s.close();
  }
});

test("ST5 a share buy counts against the session spend cap", async () => {
  const s = await writer("dave.testnet", { AGENT_EXIT_SESSION_CAP: "100" });
  try {
    const r = await s.call("buy_share", BUY);
    assert.equal(r.body.error.code, "SESSION_CAP", JSON.stringify(r.body));
    assert.equal(r.body.sent, false);
  } finally {
    await s.close();
  }
});
