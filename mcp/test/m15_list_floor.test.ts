// M15 (review 5, M4): a model talked into selling cheap is stopped by the
// operator's floor (AGENT_EXIT_MIN_LIST_PRICE), not by its own judgement.
import { test } from "node:test";
import assert from "node:assert/strict";
import { loadConfig } from "../src/config.js";
import { valid, writer } from "./helpers.js";

const list = { agent: "translator.testnet", price: 1000, confirm: true };

test("M15 a listing below the operator floor is refused before anything is sent", async () => {
  const s = await writer("alice.testnet", { AGENT_EXIT_MIN_LIST_PRICE: "2000" });
  try {
    const r = await s.call("list_for_sale", list);
    assert.ok(valid("list_for_sale", r));
    assert.equal(r.body.error.code, "UNDER_MIN_PRICE");
    assert.equal(r.body.sent, false);
    assert.match(r.body.error.message, /2000\.00/);
  } finally {
    await s.close();
  }
});

test("M15 at the floor the listing goes on to the usual checks", async () => {
  const s = await writer("alice.testnet", { AGENT_EXIT_MIN_LIST_PRICE: "1000" });
  try {
    const r = await s.call("list_for_sale", list);
    assert.notEqual(r.body.error?.code, "UNDER_MIN_PRICE", JSON.stringify(r.body).slice(0, 300));
  } finally {
    await s.close();
  }
});

test("M15 a bad floor is a start-up error", () => {
  assert.throws(() => loadConfig({ AGENT_EXIT_MIN_LIST_PRICE: "-1" }), /AGENT_EXIT_MIN_LIST_PRICE/);
  assert.equal(loadConfig({}).minListPrice, null);
});
