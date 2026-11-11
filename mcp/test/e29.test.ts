// E29: buy_agent without confirm, above max_price, or with any scan finding:
// refused before any transaction, JSON error with the reason.
import { test } from "node:test";
import assert from "node:assert/strict";
import { mkdtempSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { SpendLedger } from "../src/tools.js";
import { RUNNER_PK, valid, writer } from "./helpers.js";

const buy = (agent: string, extra: Record<string, unknown>) => ({
  agent, max_price: 1000, confirm: true, ...extra,
});

test("E29 refusals happen before anything is sent", async () => {
  const s = await writer();
  try {
    const cases: [Record<string, unknown>, string][] = [
      [buy("translator.testnet", { confirm: false }), "CONFIRM_REQUIRED"],
      [buy("translator.testnet", { max_price: 999.99 }), "OVER_MAX_PRICE"],
      [buy("cheat.testnet", { max_price: 600 }), "SCAN_BLOCKED"],
      [{ agent: "translator.testnet", confirm: true }, "BAD_INPUT"], // no max_price
      // The runner key is not a tool argument (A37).
      [buy("translator.testnet", { buyer_runner_pk: RUNNER_PK }), "BAD_INPUT"],
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

test("A40 concurrent buy_agent calls cannot each pass the session cap", async () => {
  const s = await writer("summarizer.testnet", { AGENT_EXIT_SESSION_CAP: "1500" });
  try {
    const rs = await Promise.all([s.call("buy_agent", buy("translator.testnet", {})), s.call("buy_agent", buy("translator.testnet", {}))]);
    const codes = rs.map((r) => (r.ok ? "ok" : r.body.error.code)).sort();
    // Exactly one buy goes through. Which refusal the other gets depends on
    // timing: SESSION_CAP if it reserves while the first is in flight,
    // NOT_LISTED if the first already finished. The cap itself is checked
    // deterministically in the next test.
    assert.equal(codes.filter((c) => c === "ok").length, 1, JSON.stringify(codes));
    assert.ok(codes.every((c) => ["ok", "SESSION_CAP", "NOT_LISTED"].includes(c)), JSON.stringify(codes));
  } finally {
    await s.close();
  }
});

test("A40 the spend ledger refuses a second reservation over the cap (no await between check and add)", () => {
  for (const file of [null, join(mkdtempSync(join(tmpdir(), "a40-")), "spend.json")]) {
    const l = new SpendLedger(file, "summarizer.testnet");
    l.reserve(1_000_000_000n, 1_500_000_000n);
    assert.throws(() => l.reserve(1_000_000_000n, 1_500_000_000n), (e: Error & { code?: string }) => e.code === "SESSION_CAP");
    assert.equal(l.total(), 1_000_000_000n, "the refused reservation added nothing");
    l.reserve(500_000_000n, 1_500_000_000n);
    assert.equal(l.total(), 1_500_000_000n, "up to the cap exactly is allowed");
  }
});

test("A40 a refused buy releases its reservation", async () => {
  const s = await writer("summarizer.testnet", { AGENT_EXIT_SESSION_CAP: "1000" });
  try {
    const blocked = await s.call("buy_agent", buy("cheat.testnet", { max_price: 600 }));
    assert.equal(blocked.body.error.code, "SCAN_BLOCKED");
    const r = await s.call("buy_agent", buy("translator.testnet", {}));
    assert.equal(r.ok, true, JSON.stringify(r.body));
    assert.equal(r.body.session_spent, "1000.00");
  } finally {
    await s.close();
  }
});
