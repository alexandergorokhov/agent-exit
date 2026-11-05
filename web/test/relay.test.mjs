// W5 (review 5, M6): the read-only relay under judge load. No network: every
// upstream is a mock fetch. Run: node --test web/test/relay.test.mjs
import { test } from "node:test";
import assert from "node:assert/strict";
import { relay, refuse, ARCHIVAL, clearCache } from "../rpc-allow.mjs";

const TX = { jsonrpc: "2.0", id: 7, method: "tx", params: { tx_hash: "GAPESp9FLYhSNgomeJDDgx6mPTCfsYxWrSCzz8V4eC3K", sender_account_id: "bob.agentexit3.testnet", wait_until: "NONE" } };
const Q = { jsonrpc: "2.0", id: 1, method: "query", params: { request_type: "view_account", account_id: "translator.w2.agentexit3.testnet", finality: "final" } };
const ok = (result) => ({ ok: true, status: 200, json: async () => ({ jsonrpc: "2.0", id: "x", result }) });

function mock(script) {
  const calls = [];
  const f = async (url, init) => {
    calls.push({ url, init });
    const r = script(url, calls.length);
    if (r instanceof Error) throw r;
    return r;
  };
  return { f, calls };
}

test("W5 refusals: smuggled params, wrong wait_until, non-final query, send methods", () => {
  assert.ok(refuse({ ...TX, params: { ...TX.params, extra: 1 } }));
  assert.ok(refuse({ ...TX, params: { ...TX.params, wait_until: "FINAL" } }));
  assert.ok(refuse({ ...Q, params: { ...Q.params, finality: "optimistic" } }));
  assert.ok(refuse({ jsonrpc: "2.0", id: 1, method: "send_tx", params: { signed_tx_base64: "AA", wait_until: "NONE" } }));
});

test("W5 every upstream fetch refuses redirects and has a short timeout", async () => {
  clearCache();
  const { f, calls } = mock(() => ok({ amount: "1" }));
  const r = await relay(Q, ["https://a.example"], f);
  assert.equal(r.status, 200);
  assert.equal(calls[0].init.redirect, "error");
  assert.ok(calls[0].init.signal, "a timeout signal");
});

test("W5 a 429 from the first upstream falls through to the next", async () => {
  clearCache();
  const { f, calls } = mock((url) => (url.includes("a.example") ? { ok: false, status: 429, json: async () => ({}) } : ok({ amount: "2" })));
  const r = await relay(Q, ["https://a.example", "https://b.example"], f);
  assert.equal(r.status, 200);
  assert.equal(r.json.result.amount, "2");
  assert.equal(calls.length, 2);
});

test("W5 UNKNOWN_TRANSACTION goes on to the archival node", async () => {
  clearCache();
  const { f, calls } = mock((url) =>
    url === ARCHIVAL ? ok({ status: { SuccessValue: "" } }) : { ok: true, status: 200, json: async () => ({ jsonrpc: "2.0", id: 7, error: { name: "HANDLER_ERROR", cause: { name: "UNKNOWN_TRANSACTION" } } }) });
  const r = await relay(TX, ["https://a.example"], f);
  assert.equal(r.status, 200);
  assert.ok(r.json.result);
  assert.equal(calls.at(-1).url, ARCHIVAL);
});

test("W5 a finished transaction is cached: a second judge's request does not hit the upstream", async () => {
  clearCache();
  const { f, calls } = mock(() => ok({ status: { SuccessValue: "" } }));
  const a = await relay(TX, ["https://a.example"], f);
  const b = await relay({ ...TX, id: 99 }, ["https://a.example"], f);
  assert.equal(calls.length, 1);
  assert.equal(b.json.id, 99, "the cached answer carries the caller's id");
  assert.deepEqual(b.json.result, a.json.result);
});

test("W5 errors are not cached", async () => {
  clearCache();
  let n = 0;
  const { f, calls } = mock(() => (++n === 1 ? new Error("ECONNRESET") : ok({ amount: "3" })));
  const a = await relay(Q, ["https://a.example"], f);
  assert.equal(a.status, 502);
  const b = await relay(Q, ["https://a.example"], f);
  assert.equal(b.status, 200);
  assert.equal(calls.length, 2);
});

test("W5 the relay's own error text never echoes upstream bodies", async () => {
  clearCache();
  const { f } = mock(() => new Error("<script>alert(1)</script>".repeat(50)));
  const r = await relay(Q, ["https://a.example"], f);
  assert.equal(r.status, 502);
  assert.ok(r.json.error.message.length <= 300, r.json.error.message.length);
});
