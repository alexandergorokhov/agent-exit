// RR1-RR2 (judge-testnet K5, 2026-11-07): rpc.testnet.near.org answered HTTP
// 429 to every verify_lockout read, so `make judge-testnet` failed K5 on a
// busy public node. Reads retry 429 and 5xx; a contract view that still fails
// on the first provider is asked of the second (views only: the key lists the
// scan compares are still read from each provider separately).
import { test } from "node:test";
import assert from "node:assert/strict";
import { RETRY_429_MS, TestnetBackend } from "../src/backend.js";
import { loadConfig } from "../src/config.js";

RETRY_429_MS.splice(0, RETRY_429_MS.length, 1, 1);
const AGENT = "translator.testnet";
const RPC1 = "https://rpc.testnet.near.org";
const RPC2 = "https://test.rpc.fastnear.com";
const STATUS = { status: "Active", owner: "bob.testnet" };

function chain(statusFor: (url: string, n: number) => number) {
  const asked: string[] = [];
  const f = (async (url: string, init?: RequestInit) => {
    asked.push(url);
    const s = statusFor(url, asked.filter((u) => u === url).length);
    if (s !== 200) return new Response("busy", { status: s });
    const p = JSON.parse(String(init?.body)).params;
    const result = p.request_type === "call_function" ? { result: [...Buffer.from(JSON.stringify(STATUS))] } : { keys: [] };
    return new Response(JSON.stringify({ result }), { status: 200 });
  }) as unknown as typeof fetch;
  return { f, asked };
}
const backend = (f: typeof fetch) => new TestnetBackend(loadConfig({ AGENT_EXIT_NETWORK: "testnet" }), f);
const view = (b: TestnetBackend) => (b as unknown as { view(id: string, m: string): Promise<unknown> }).view(AGENT, "get_status");

test("RR1 a 503 is retried like a 429", async () => {
  const c = chain((_u, n) => (n === 1 ? 503 : 200));
  assert.deepEqual(await view(backend(c.f)), STATUS);
  assert.deepEqual(c.asked, [RPC1, RPC1]);
});

test("RR1 a 400 is not retried", async () => {
  const c = chain((u) => (u === RPC1 ? 400 : 200));
  await view(backend(c.f));
  assert.equal(c.asked.filter((u) => u === RPC1).length, 1);
});

test("RR2 a view the first provider keeps refusing is read from the second", async () => {
  const c = chain((u) => (u === RPC1 ? 429 : 200));
  assert.deepEqual(await view(backend(c.f)), STATUS);
  assert.equal(c.asked.filter((u) => u === RPC1).length, 3, "first provider: 1 try + 2 retries");
  assert.equal(c.asked.at(-1), RPC2);
});

test("RR2 both providers down is still an RPC_ERROR", async () => {
  const c = chain(() => 429);
  await assert.rejects(view(backend(c.f)), (e: { code: string }) => e.code === "RPC_ERROR");
});
