// AI1: the scripted investing agent picks by counted revenue per 1% of price,
// skips offers it cannot buy, and its offline run buys and shows a holding.
import { test } from "node:test";
import assert from "node:assert/strict";
import { execFileSync } from "node:child_process";
import { mkdtempSync, readFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { pick } from "../scripts/agent-invests.js";
import { REPO } from "./helpers.js";

const offer = (id: number, ppp: string, open = 10, min = 1, owner = true) =>
  ({ offer_id: id, pct: 10, price: "0", price_per_pct: ppp, open_pct: open, min_part_pct: min, seller_is_owner: owner });

test("AI1 pick: best counted revenue per dollar, only buyable offers with 2+ payers", () => {
  const agents = [
    { agent: "a.testnet", counted: "100.00", counted_payers: 3, buyable: true, offers: [offer(1, "10.00")] },
    { agent: "b.testnet", counted: "300.00", counted_payers: 3, buyable: true, offers: [offer(2, "15.00")] },
    { agent: "c.testnet", counted: "900.00", counted_payers: 1, buyable: true, offers: [offer(3, "1.00")] },
    { agent: "d.testnet", counted: "900.00", counted_payers: 5, buyable: false, offers: [offer(4, "1.00")] },
    { agent: "e.testnet", counted: "900.00", counted_payers: 5, buyable: true, offers: [offer(5, "1.00", 1, 2)] },
    { agent: "f.testnet", counted: "900.00", counted_payers: 5, buyable: true, offers: [offer(6, "1.00", 10, 1, false)] },
  ];
  const p = pick(agents);
  assert.equal(p?.agent, "b.testnet");
  assert.equal(p?.offer.offer_id, 2);
  assert.equal(pick([]), null);
});

test("AI1 offline run: buys a part through MCP and the portfolio shows it", () => {
  const out = join(mkdtempSync(join(tmpdir(), "agent-invests-")), "o.json");
  execFileSync(process.execPath, [join(REPO, "mcp", "dist", "scripts", "agent-invests.js"), "--out", out], { cwd: REPO, stdio: "pipe" });
  const r = JSON.parse(readFileSync(out, "utf8"));
  assert.equal(r.pass, true);
  assert.equal(r.all_schema_valid, true);
  assert.deepEqual(r.steps.map((s: { tool: string }) => s.tool), ["list_offers", "get_earnings", "buy_share", "portfolio"]);
});
