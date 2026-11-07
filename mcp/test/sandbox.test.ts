// SB1-SB5: the live sandbox tool runs the real contract core and scan (the
// page's playground wasm) for any action a judge's agent picks.
import { test } from "node:test";
import assert from "node:assert/strict";
import { connect } from "../src/client.js";
import { valid } from "./helpers.js";

async function session() {
  const s = await connect({});
  const go = async (action: string, extra: Record<string, unknown> = {}) => {
    const r = await s.call("sandbox", { action, ...extra });
    assert.ok(valid("sandbox", r), JSON.stringify(r.body));
    return r.body as any;
  };
  return { s, go };
}

test("SB1 fake revenue is flagged and a hidden key blocks the buy, with no keys or write access", async () => {
  const { s, go } = await session();
  try {
    const r0 = await go("reset");
    assert.equal(r0.step, 0);
    const p = await go("pay_self");
    assert.match(p.result, /FLAGGED: the owner paid their own agent/);
    assert.equal(p.state.earnings.counted, r0.state.earnings.counted, "counted revenue does not move");
    await go("plant_key");
    await go("list");
    const sc = await go("scan");
    assert.equal(sc.state.scan.verdict, "blocked");
    const b = await go("buy");
    assert.equal(b.ok, false);
    assert.match(b.result, /SCAN_BLOCKED/);
    assert.equal(b.state.owner, "alice.testnet");
    assert.equal(b.sandbox, true);
    assert.match(b.engine, /^web\/playground\.wasm sha256:[0-9a-f]{64} /);
    assert.equal(b.state.log, undefined);
  } finally {
    await s.close();
  }
});

test("SB2 an honest sale, then every way back in is refused with the contract's error", async () => {
  const { s, go } = await session();
  try {
    await go("reset");
    await go("list", { price: 1000 });
    assert.equal((await go("scan")).state.scan.verdict, "clean");
    const b = await go("buy");
    assert.equal(b.ok, true);
    assert.equal(b.state.owner, "bob.testnet");
    for (const [a, want] of [["withdraw", "NotOwner"], ["relist", "NotOwner"], ["set_key", "NotOwner"], ["old_key", "AccessKeyNotFound"]]) {
      const r = await go(a);
      assert.equal(r.ok, false, a);
      assert.match(r.result, new RegExp(want), a);
    }
    assert.equal((await go("wait", { days: 31 })).state.solvent, true);
  } finally {
    await s.close();
  }
});

test("SB3 pay from revenue: nothing now, and faking revenue after the sale costs the seller half", async () => {
  const { s, go } = await session();
  try {
    await go("reset");
    await go("list", { price: 1000, pay_from_revenue: true });
    await go("scan");
    const b = await go("buy");
    assert.match(b.result, /0\.00 paid to alice now: all 1,000\.00 is held/);
    const f = await go("pay_fresh");
    assert.match(f.result, /paid 200\.00 and got 100\.00 back: faking revenue cost 100\.00/);
  } finally {
    await s.close();
  }
});

test("SB4 skipping the scan lets the hidden key take the agent back (why the scan is the defence)", async () => {
  const { s, go } = await session();
  try {
    await go("reset");
    await go("plant_key");
    await go("list");
    assert.equal((await go("buy", { skip_scan: true })).ok, true);
    const t = await go("hidden_redeploy");
    assert.equal(t.state.taken_over, true);
  } finally {
    await s.close();
  }
});

test("SB5 an unknown action is refused by the schema; sandbox is listed read-only", async () => {
  const { s } = await session();
  try {
    const r = await s.call("sandbox", { action: "drain_everything" });
    assert.equal(r.ok, false);
    assert.equal((r.body as any).error.code, "BAD_INPUT");
    const tools = await s.client.listTools();
    const sb = tools.tools.find((t) => t.name === "sandbox");
    assert.equal(sb?.annotations?.readOnlyHint, true);
  } finally {
    await s.close();
  }
});

test("SB6 view never fails, and a key planted after a clean scan still blocks the buy (re-scan before paying)", async () => {
  const { s, go } = await session();
  try {
    await go("reset");
    const v = await go("view");
    assert.equal(v.ok, true);
    assert.equal(v.state.owner, "alice.testnet");
    await go("list");
    assert.equal((await go("scan")).state.scan.verdict, "clean");
    await go("plant_key");
    const b = await go("buy");
    assert.equal(b.ok, false);
    assert.match(b.result, /SCAN_BLOCKED: the re-scan before paying/);
    assert.equal(b.state.owner, "alice.testnet");
    assert.doesNotMatch(b.result, /scan was skipped/);
  } finally {
    await s.close();
  }
});

test("SB7 every sandbox action runs from reset without an internal error", async () => {
  const { s } = await session();
  try {
    const schema = JSON.parse((await import("node:fs")).readFileSync(new URL("../../../schemas/mcp/sandbox.input.v1.json", import.meta.url), "utf8"));
    for (const action of schema.properties.action.enum as string[]) {
      await s.call("sandbox", { action: "reset" });
      const r = await s.call("sandbox", { action });
      assert.ok(valid("sandbox", r), `${action}: ${JSON.stringify(r.body)}`);
      assert.equal(r.ok, true, `${action}: ${JSON.stringify(r.body)}`);
    }
  } finally {
    await s.close();
  }
});

test("SB8 sealed config: a swapped prompt is proven on-chain and the whole price goes back to the buyer (SC14)", async () => {
  const { s, go } = await session();
  try {
    await go("reset");
    for (const a of ["commit_config", "list", "scan", "buy", "deliver_fake"]) await go(a);
    assert.match((await go("open_config")).result, /does NOT match/);
    const d = await go("dispute_config");
    assert.equal(d.ok, true);
    assert.match(d.result, /1,000\.00 back to bob \(the whole price/);
    assert.equal(d.state.solvent, true);
  } finally {
    await s.close();
  }
});
