// Never mainnet; testnet mode is read-only and gated by an env var.
import { test } from "node:test";
import assert from "node:assert/strict";
import { spawnSync } from "node:child_process";
import { SERVER } from "../src/client.js";
import { assertNotMainnet, loadConfig } from "../src/config.js";
import { TestnetBackend, wasmExports } from "../src/backend.js";

test("mainnet is refused at startup", () => {
  for (const env of [
    { AGENT_EXIT_NETWORK: "mainnet" },
    { AGENT_EXIT_NETWORK: "testnet", AGENT_EXIT_RPC: "https://rpc.mainnet.near.org" },
    { AGENT_EXIT_NETWORK: "testnet", AGENT_EXIT_RPC_2: "http://localhost:3030" },
    { AGENT_EXIT_NETWORK: "testnet", AGENT_EXIT_AGENTS: "translator.near" },
  ]) {
    const r = spawnSync(process.execPath, [SERVER], { env: { PATH: process.env.PATH, ...env }, encoding: "utf8", input: "" });
    assert.equal(r.status, 2, `${JSON.stringify(env)}: ${r.stderr}`);
    assert.match(r.stderr, /refus|not a \.testnet/);
  }
  assert.throws(() => assertNotMainnet("https://free.rpc.fastnear.com/mainnet"));
  // A41: mainnet endpoints whose URL does not say "mainnet" are refused too.
  for (const u of ["https://rpc.fastnear.com", "https://free.rpc.fastnear.com", "https://near.lava.build", "https://rpc.near.org", "https://rpc.testnet.near.org.evil.com", "https://user@rpc.testnet.near.org", "http://rpc.testnet.near.org"]) {
    assert.throws(() => assertNotMainnet(u), new RegExp("refusing"), u);
  }
  for (const u of ["https://rpc.testnet.near.org", "https://test.rpc.fastnear.com"]) assertNotMainnet(u);
});

test("default is offline", () => {
  assert.equal(loadConfig({}).network, "offline");
  assert.equal(loadConfig({ AGENT_EXIT_NETWORK: "testnet" }).network, "testnet");
});

test("testnet writes say not built and send nothing", async () => {
  const b = new TestnetBackend(loadConfig({ AGENT_EXIT_NETWORK: "testnet" }));
  await assert.rejects(b.buy(), (e: { code: string; message: string }) => e.code === "NOT_BUILT" && /not built yet/.test(e.message));
  await assert.rejects(b.list(), (e: { code: string }) => e.code === "NOT_BUILT");
});

test("wasm export parser reads function exports", () => {
  // (module (func) (export "get_status" (func 0)) (export "pay" (func 0)))
  const m = Uint8Array.from([
    0, 0x61, 0x73, 0x6d, 1, 0, 0, 0,
    1, 4, 1, 0x60, 0, 0,
    3, 2, 1, 0,
    7, 0x14, 2, 10, ...Buffer.from("get_status"), 0, 0, 3, ...Buffer.from("pay"), 0, 0,
    10, 4, 1, 2, 0, 0x0b,
  ]);
  assert.deepEqual(wasmExports(m), ["get_status", "pay"]);
});
