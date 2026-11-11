// K0: `make judge` checks that the committed wasm's code hash is the one the
// testnet records name, and fails when any copy differs.
import { test } from "node:test";
import assert from "node:assert/strict";
import { mkdtempSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { base58, checkCodeHash, k0Commands } from "../scripts/k0.js";
import { REPO } from "./helpers.js";

test("K0 base58 matches NEAR's encoding", () => {
  assert.equal(base58(new Uint8Array([0, 0, 1])), "112");
  assert.equal(base58(Buffer.from("hello")), "Cn8eVZg");
});

test("K0 the committed wasm matches testnet.json, deploy.json, .mcp.json and SHA256SUMS", () => {
  const k = checkCodeHash(REPO);
  assert.deepEqual(k.mismatches, []);
  assert.equal(k.pass, true);
  assert.equal(k.hash, JSON.parse(readFileSync(join(REPO, "web", "data", "testnet.json"), "utf8")).agent_code_hash);
  const [curl, nep330] = k0Commands(k);
  assert.match(curl, /view_account/);
  assert.match(curl, new RegExp(k.hash));
  assert.match(nep330, /contract_source_metadata/);
});

test("K0 fails when one record names another hash", () => {
  const dir = mkdtempSync(join(tmpdir(), "k0-"));
  try {
    const bad = join(dir, "deploy.json");
    const d = JSON.parse(readFileSync(join(REPO, "out", "testnet", "deploy.json"), "utf8"));
    d.agent_code_hash = `${d.agent_code_hash.slice(0, -1)}X`;
    writeFileSync(bad, JSON.stringify(d));
    const k = checkCodeHash(REPO, { deploy: bad });
    assert.equal(k.pass, false);
    assert.match(k.mismatches.join(), /deploy\.json agent_code_hash/);
  } finally {
    rmSync(dir, { recursive: true, force: true });
  }
});
