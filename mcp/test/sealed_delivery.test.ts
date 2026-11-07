// SD1-SD6: the buyer's side of a sealed config through MCP (sealed_delivery):
// read the delivery, open it with the one-time key, accept or dispute.
// Offline fixtures: fixtures/mcp/sealed.json (scripts/gen-sealed-fixture.mjs).
import { test } from "node:test";
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { connect } from "../src/client.js";
import { valid, writer } from "./helpers.js";

test("SD1 status and open are read-only: an honest delivery opens to the committed bundle", async () => {
  const s = await connect({});
  try {
    const st = await s.call("sealed_delivery", { agent: "brainy.testnet", sale_id: 1, action: "status" });
    assert.ok(valid("sealed_delivery", st), JSON.stringify(st.body));
    assert.equal((st.body as any).state, "delivered");
    const o = await s.call("sealed_delivery", { agent: "brainy.testnet", sale_id: 1, action: "open" });
    assert.ok(valid("sealed_delivery", o), JSON.stringify(o.body));
    const b: any = o.body;
    assert.equal(b.matches, true);
    assert.deepEqual(b.untrusted_rotate_after_sale, ["NEARAI_API_KEY"]);
    assert.ok(!JSON.stringify(b).includes("2a2a2a2a"), "the secret is never in the output");
  } finally {
    await s.close();
  }
});

test("SD2 a swapped prompt opens as a mismatch", async () => {
  const s = await connect({});
  try {
    const o: any = (await s.call("sealed_delivery", { agent: "swapped.testnet", sale_id: 1, action: "open" })).body;
    assert.equal(o.matches, false);
    assert.match(o.next, /dispute/);
  } finally {
    await s.close();
  }
});

test("SD3 accept and dispute are writes: refused without write mode or confirm", async () => {
  const s = await connect({});
  try {
    const r = await s.call("sealed_delivery", { agent: "brainy.testnet", sale_id: 1, action: "accept", confirm: true });
    assert.equal((r.body as any).error.code, "WRITE_DISABLED");
  } finally {
    await s.close();
  }
  const w = await writer("summarizer.testnet");
  try {
    const r = await w.call("sealed_delivery", { agent: "brainy.testnet", sale_id: 1, action: "accept" });
    assert.equal((r.body as any).error.code, "CONFIRM_REQUIRED");
  } finally {
    await w.close();
  }
});

test("SD4 the wrong move is refused before sending: no accept of a swap, no dispute of a match", async () => {
  const w = await writer("summarizer.testnet");
  try {
    const a = await w.call("sealed_delivery", { agent: "swapped.testnet", sale_id: 1, action: "accept", confirm: true });
    assert.equal((a.body as any).error.code, "CONFIG_MISMATCH");
    const d = await w.call("sealed_delivery", { agent: "brainy.testnet", sale_id: 1, action: "dispute", confirm: true });
    assert.equal((d.body as any).error.code, "CONFIG_MATCHES");
  } finally {
    await w.close();
  }
});

test("SD5 accepting an honest delivery pays the seller 690.00 (700 less the 1% fee)", async () => {
  const w = await writer("summarizer.testnet");
  try {
    const r = await w.call("sealed_delivery", { agent: "brainy.testnet", sale_id: 1, action: "accept", confirm: true });
    assert.ok(valid("sealed_delivery", r), JSON.stringify(r.body));
    const b: any = r.body;
    assert.equal(b.state, "accepted");
    assert.equal(b.to_seller, "690.00");
    const again: any = (await w.call("sealed_delivery", { agent: "brainy.testnet", sale_id: 1, action: "status" })).body;
    assert.equal(again.state, "accepted");
  } finally {
    await w.close();
  }
});

test("SD6 disputing a swapped prompt returns the whole price; only the buyer may dispute", async () => {
  const other = await writer("research.testnet");
  try {
    const r = await other.call("sealed_delivery", { agent: "swapped.testnet", sale_id: 1, action: "dispute", confirm: true });
    assert.equal((r.body as any).error.code, "CONTRACT_REFUSED");
    assert.match((r.body as any).error.message, /NotBuyer/);
  } finally {
    await other.close();
  }
  const w = await writer("summarizer.testnet");
  try {
    const r = await w.call("sealed_delivery", { agent: "swapped.testnet", sale_id: 1, action: "dispute", confirm: true });
    assert.ok(valid("sealed_delivery", r), JSON.stringify(r.body));
    assert.equal((r.body as any).refund_to_buyer, "1000.00");
  } finally {
    await w.close();
  }
});

test("SD7 someone who is not the buyer cannot open the bundle; each fixture has its own secret", async () => {
  const m = await connect({ AGENT_EXIT_ACCOUNT_ID: "mallory.testnet" });
  try {
    const r = await m.call("sealed_delivery", { agent: "swapped.testnet", sale_id: 1, action: "open" });
    assert.equal(r.ok, false);
    assert.match((r.body as any).error.message, /NotBuyer/);
    const st = await m.call("sealed_delivery", { agent: "swapped.testnet", sale_id: 1, action: "status" });
    assert.ok(!JSON.stringify(st.body).includes("demo_buyer_sk"), "the fixture secret never leaves the backend");
  } finally {
    await m.close();
  }
  const fx = JSON.parse(readFileSync(new URL("../../../fixtures/mcp/sealed.json", import.meta.url), "utf8"));
  assert.notEqual(fx.agents["brainy.testnet"].demo_buyer_sk, fx.agents["swapped.testnet"].demo_buyer_sk);
});
