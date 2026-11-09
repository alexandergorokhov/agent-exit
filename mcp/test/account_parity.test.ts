// CA13: the demo page's "Check any NEAR account" box (web/account-check.mjs)
// and the MCP tool check_account give the same verdict on every offline
// account: the Agent Exit agents and the illustrative fixtures.
import { test } from "node:test";
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { join } from "node:path";
import { connect } from "../src/client.js";
import { REPO, world } from "./helpers.js";

test("CA13 the page's checker and check_account agree on every offline account", async () => {
  const page = await import(join(REPO, "web", "account-check.mjs"));
  const extra = JSON.parse(readFileSync(join(REPO, "fixtures", "mcp", "accounts.json"), "utf8")).accounts;
  const facts: Record<string, any> = {};
  for (const [id, a] of Object.entries<any>(world.agents)) {
    const c = a.scan_input.chain;
    facts[id] = { account: id, code_hash: c.code_hash, methods: c.methods, keys: c.keys, known_keys: a.status.known_keys };
  }
  for (const [id, a] of Object.entries<any>(extra)) facts[id] = { account: id, code_hash: a.code_hash, methods: a.methods, keys: a.keys, known_keys: null, global_contract_account_id: a.global_contract_account_id ?? null, global_contract_hash: a.global_contract_hash ?? null };
  const s = await connect({});
  try {
    for (const [id, f] of Object.entries(facts)) {
      const mcp: any = (await s.call("check_account", { account: id })).body;
      const p = page.assess(f, { publishedHash: "fixture-release-hash-v1" });
      assert.equal(p.verdict, mcp.verdict, id);
    }
  } finally {
    await s.close();
  }
  assert.ok(Object.keys(facts).length >= 6);
});
