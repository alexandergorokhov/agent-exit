// Review 2 M2: an agent whose earnings are already owed to investors.
// SB1 above the server's AGENT_EXIT_MAX_SHARES_BPS: refused, nothing sent.
// SB2 under it: bought, and the result carries shares_pct and the scan's
// shares_disclosed warning; list_agents shows shares_pct.
import { test } from "node:test";
import assert from "node:assert/strict";
import { mkdtempSync, readFileSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { REPO, valid, writer } from "./helpers.js";

function world(bps: number): string {
  const w = JSON.parse(readFileSync(join(REPO, "fixtures", "mcp", "world.json"), "utf8"));
  const a = w.agents["translator.testnet"];
  const end = String(BigInt(Date.now()) * 1_000_000n + 200n * 86_400_000_000_000n);
  const slice = { offer_id: 1, seller: "alice.testnet", bps, price: "100000000", created: "0", end, sold_bps: bps,
    holders: [{ account: "dave.testnet", bps, paid_in: "100000000", escrow: "50000000", earned: "0" }] };
  a.status.shares_bps = bps;
  a.listing.shares_bps = bps;
  a.listing.shares = [slice];
  a.scan_input.listing.shares_bps = bps;
  a.scan_input.listing.shares = [slice];
  const dir = mkdtempSync(join(tmpdir(), "agent-exit-sb-"));
  const p = join(dir, "world.json");
  writeFileSync(p, JSON.stringify(w));
  return p;
}

test("SB1 an agent with more than the server's limit owed to investors is not bought", async () => {
  const s = await writer("summarizer.testnet", { AGENT_EXIT_WORLD: world(3000) });
  try {
    const r = await s.call("buy_agent", { agent: "translator.testnet", max_price: 1000, confirm: true });
    assert.ok(valid("buy_agent", r));
    assert.equal(r.body.error.code, "SHARES_OVER_MAX");
    assert.equal(r.body.sent, false);
  } finally {
    await s.close();
  }
});

test("SB2 under the limit it buys, and says what is owed to investors", async () => {
  const s = await writer("summarizer.testnet", { AGENT_EXIT_WORLD: world(1000) });
  try {
    const l = await s.call("list_agents");
    assert.ok(valid("list_agents", l));
    assert.equal(l.body.agents.find((a: { agent: string }) => a.agent === "translator.testnet").shares_pct, 10);
    const r = await s.call("buy_agent", { agent: "translator.testnet", max_price: 1000, confirm: true });
    assert.ok(valid("buy_agent", r), JSON.stringify(r.body));
    assert.equal(r.ok, true, JSON.stringify(r.body));
    assert.equal(r.body.shares_pct, 10);
    assert.ok(r.body.warnings.some((w: { code: string }) => w.code === "shares_disclosed"), JSON.stringify(r.body.warnings));
  } finally {
    await s.close();
  }
});
