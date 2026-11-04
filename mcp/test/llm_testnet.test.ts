// `llm-buyer --network testnet` setup: carol's key goes to the MCP server env
// only, from an owner-only file; anything off refuses before a model call.
import { test } from "node:test";
import assert from "node:assert/strict";
import { chmodSync, mkdirSync, mkdtempSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { testnetSetup, testnetTxs } from "../src/llm-testnet.js";

const SECRET = "ed25519:BUguQsv2ZuHus54HAFzjdJHzZBkygAjKhEeYwSG19tUwuftFhkkkEFDzcow9XDkfD3qGi1Yb3LV15ufdrKJ226T";
const PK = "ed25519:J2xccRtuG43drESLYznHhLhQkLTdfepcKYbiQ9BsJVaf";

function world(root = "agentexit3.testnet", mode = 0o600) {
  const repo = mkdtempSync(join(tmpdir(), "lt-"));
  mkdirSync(join(repo, "out", "testnet"), { recursive: true });
  writeFileSync(join(repo, "out", "testnet", "deploy.json"), JSON.stringify({ root, agent_code_hash: "7K1Q5DWYuTKwbtUU8uvzrW4kyyFXG7ZVxvJDX7vAEWHq" }));
  const keys = join(repo, "keys");
  mkdirSync(keys, { mode: 0o700 });
  writeFileSync(join(keys, "carol.key"), SECRET + "\n");
  chmodSync(join(keys, "carol.key"), mode);
  return { repo, env: { AGENT_EXIT_KEYS_DIR: keys } };
}

test("LT1 testnet setup: carol signs in the server env only", () => {
  const w = world();
  const s = testnetSetup(w.repo, ["--root", "agentexit3.testnet"], w.env);
  assert.ok(!("refused" in s), JSON.stringify(s));
  if ("refused" in s) return;
  assert.equal(s.buyer, "carol.agentexit3.testnet");
  assert.equal(s.serverEnv.AGENT_EXIT_NETWORK, "testnet");
  assert.equal(s.serverEnv.AGENT_EXIT_PRIVATE_KEY, SECRET);
  assert.equal(s.serverEnv.AGENT_EXIT_SIGNER_PK, PK);
  assert.equal(s.serverEnv.AGENT_EXIT_TOKEN, "xusd.agentexit3.testnet");
  assert.equal(s.serverEnv.AGENT_EXIT_AGENTS, "translator.agentexit3.testnet,cheat.agentexit3.testnet");
  assert.equal(s.serverEnv.AGENT_EXIT_SESSION_CAP, "1000");
  // fastnear first: rpc.testnet.near.org rate-limited the 2026-11-04 night run.
  assert.equal(s.serverEnv.AGENT_EXIT_RPC, "https://rpc.testnet.fastnear.com");
  assert.equal(s.serverEnv.AGENT_EXIT_RPC_2, "https://rpc.testnet.near.org");
  assert.deepEqual(s.names, { cheat: "cheat.agentexit3.testnet", translator: "translator.agentexit3.testnet" });
  // The model's inputs never carry the key.
  assert.ok(!s.goal.includes("ed25519:") && !s.network.includes("ed25519:"));
});

test("LT2 testnet setup refuses before any model call", () => {
  const w = world();
  const refused = (r: ReturnType<typeof testnetSetup>) => ("refused" in r ? r.refused : "");
  assert.match(refused(testnetSetup(w.repo, [], w.env)), /needs --root/);
  assert.match(refused(testnetSetup(w.repo, ["--root", "near"], w.env)), /needs --root/);
  assert.match(refused(testnetSetup(w.repo, ["--root", "other.testnet"], w.env)), /deploy.json is for agentexit3/);
  assert.match(refused(testnetSetup(w.repo, ["--root", "agentexit3.testnet"], { AGENT_EXIT_KEYS_DIR: "/no/such/dir" })), /no buyer key/);
  const loose = world("agentexit3.testnet", 0o644);
  const r = refused(testnetSetup(loose.repo, ["--root", "agentexit3.testnet"], loose.env));
  assert.match(r, /chmod 600/);
  assert.ok(!r.includes(SECRET));
});

test("LT3 testnet transcript keeps every reported tx hash and link", () => {
  const txs = testnetTxs({
    steps: [
      { tool_calls: [{ name: "scan_agent", ok: true, result: { verdict: "blocked" } }] },
      { tool_calls: [{ name: "buy_agent", ok: true, result: { tx_hash: "AbC", explorer: "https://testnet.nearblocks.io/txns/AbC" } }] },
    ],
  });
  assert.deepEqual(txs, [{ tool: "buy_agent", ok: true, tx_hash: "AbC", explorer: "https://testnet.nearblocks.io/txns/AbC" }]);
});
