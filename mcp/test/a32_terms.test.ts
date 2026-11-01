// A32 (review 2, H2): buy_agent takes price, listing version and earn-out
// terms from one read, refuses terms below the minimum, and pins the quoted
// version through the scan to the contract.
import { test } from "node:test";
import assert from "node:assert/strict";
import { mkdtempSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { RUNNER_PK, valid, world, writer } from "./helpers.js";

const buy = (extra: Record<string, unknown> = {}) => ({
  agent: "translator.testnet", max_price: 1000, confirm: true, buyer_runner_pk: RUNNER_PK, ...extra,
});

/** A copy of the offline world with translator changed by `edit`. */
function worldWith(edit: (t: any) => void): string {
  const w = structuredClone(world);
  edit(w.agents["translator.testnet"]);
  const dir = mkdtempSync(join(tmpdir(), "agent-exit-a32-"));
  const p = join(dir, "world.json");
  writeFileSync(p, JSON.stringify(w));
  return p;
}

function setTerms(t: any, terms: Partial<Record<string, number>>): void {
  Object.assign(t.listing.terms, terms);
  Object.assign(t.scan_input.listing.terms, terms);
}

test("A32 terms below the published minimum are refused before anything is sent", async () => {
  const cases: Record<string, number>[] = [{ earnout_bps: 0 }, { floor_bps: 10_000 }, { window_secs: 60 }];
  for (const terms of cases) {
    const s = await writer("summarizer.testnet", { AGENT_EXIT_WORLD: worldWith((t) => setTerms(t, terms)) });
    try {
      const r = await s.call("buy_agent", buy());
      assert.ok(valid("buy_agent", r));
      assert.equal(r.body.error.code, "TERMS_BELOW_MINIMUM", JSON.stringify(terms));
      assert.equal(r.body.sent, false);
    } finally {
      await s.close();
    }
  }
});

test("A32 a buyer can only tighten the minimum", async () => {
  const s = await writer();
  try {
    const r = await s.call("buy_agent", buy({ min_escrow_pct: 40 }));
    assert.equal(r.body.error.code, "TERMS_BELOW_MINIMUM");
    // Asking for less than the server minimum changes nothing.
    const s2 = await writer("summarizer.testnet", { AGENT_EXIT_WORLD: worldWith((t) => setTerms(t, { window_secs: 86_400 })) });
    try {
      const r2 = await s2.call("buy_agent", buy({ min_window_days: 1 }));
      assert.equal(r2.body.error.code, "TERMS_BELOW_MINIMUM");
    } finally {
      await s2.close();
    }
  } finally {
    await s.close();
  }
});

test("A32 a relist between the quote and the scan blocks the buy", async () => {
  // get_listing quoted version 1; by scan time the seller relisted (version 2).
  const p = worldWith((t) => {
    t.scan_input.contract.listing_version = 2;
    t.scan_input.listing.listing_version = 2;
  });
  const s = await writer("summarizer.testnet", { AGENT_EXIT_WORLD: p });
  try {
    const r = await s.call("buy_agent", buy());
    assert.equal(r.body.error.code, "SCAN_BLOCKED");
    const codes = r.body.error.details.findings.map((f: { code: string }) => f.code);
    assert.ok(codes.includes("listing_version_mismatch"), JSON.stringify(codes));
  } finally {
    await s.close();
  }
});

test("A32 the bought terms are reported and labelled prod or demo", async () => {
  const s = await writer();
  try {
    const r = await s.call("buy_agent", buy());
    assert.ok(valid("buy_agent", r));
    assert.equal(r.ok, true, JSON.stringify(r.body));
    assert.deepEqual(r.body.terms, { escrow_pct: 30, release_share_pct: 50, seller_floor_pct: 20, window_days: 30 });
    assert.equal(r.body.terms_config, "prod");
  } finally {
    await s.close();
  }
  const demo = await writer("summarizer.testnet", {
    AGENT_EXIT_MIN_WINDOW_SECS: "600",
    AGENT_EXIT_WORLD: worldWith((t) => setTerms(t, { window_secs: 600 })),
  });
  try {
    const r = await demo.call("buy_agent", buy());
    assert.equal(r.ok, true, JSON.stringify(r.body));
    assert.equal(r.body.terms_config, "demo");
    assert.equal(r.body.terms.window_days, 600 / 86_400);
  } finally {
    await demo.close();
  }
});
