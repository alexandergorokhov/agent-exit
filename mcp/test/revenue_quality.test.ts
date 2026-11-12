// N3: payer age and spread on get_earnings (revenue_quality).
import { test } from "node:test";
import assert from "node:assert/strict";
import { revenueQuality } from "../src/tools.js";
import { connect } from "../src/client.js";
import { valid } from "./helpers.js";

test("RQ1 payments from payers first seen in the last 7 days count half", () => {
  const q = revenueQuality({ counted: "360000000", counted_from_fresh_payers: "360000000", counted_payers: 6, days_total: 1 });
  assert.equal(q.age_weighted_counted, "180.00");
  assert.equal(q.from_new_payers_pct, 100);
  assert.equal(q.avg_counted_per_payer, "60.00");
  assert.equal(q.warnings.length, 3);
});

test("RQ2 old, spread-out revenue keeps its full weight and has no warnings", () => {
  const q = revenueQuality({ counted: "1000000000", counted_from_fresh_payers: "100000000", counted_payers: 25, days_total: 40 });
  assert.equal(q.age_weighted_counted, "950.00");
  assert.equal(q.from_new_payers_pct, 10);
  assert.deepEqual(q.warnings, []);
});

test("RQ3 get_earnings returns revenue_quality, schema-valid", async () => {
  const s = await connect({});
  try {
    const r = await s.call("get_earnings", { agent: "translator.factory.testnet" });
    assert.ok(valid("get_earnings", r));
    assert.equal(r.body.revenue_quality.counted_payers, 6);
    assert.match(r.body.revenue_quality.rule, /count half/);
  } finally {
    await s.close();
  }
});
