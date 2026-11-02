// Safety rules of the G2 relay (scenario ids T-S1..T-S5): testnet only,
// secrets never echoed, DRY_RUN sends nothing. Offline; viem is not needed
// because every refusal happens before it is imported.
import { test } from "node:test";
import assert from "node:assert/strict";
import { spawnSync } from "node:child_process";
import { mkdtempSync, readFileSync, writeFileSync, existsSync } from "node:fs";
import { tmpdir } from "node:os";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";
import { checkAuthorization, checkBaseRpc, checkRelayerKey, parseEnvFile, scrub } from "../g2-guards.mjs";

const RELAY = join(dirname(fileURLToPath(import.meta.url)), "..", "g2-relay.mjs");
const KEY = "0x" + "ab12".repeat(16);
const PATHKEY = "super-secret-rpc-token-123456";

function relay(env, signJson) {
  const dir = mkdtempSync(join(tmpdir(), "g2-"));
  const sign = join(dir, "g2-sign.json");
  if (signJson) writeFileSync(sign, JSON.stringify(signJson));
  const r = spawnSync(process.execPath, [RELAY, sign], { env: { PATH: process.env.PATH, AGENT_EXIT_ENV: "/nonexistent", ...env }, encoding: "utf8" });
  const outFile = join(dir, "g2.json");
  return { r, out: r.stdout + r.stderr, written: existsSync(outFile) ? readFileSync(outFile, "utf8") : "" };
}
const GOOD_AUTH = { calldata: "0xe3ee160e" + "00".repeat(64), value: "1000000", from: "0x" + "11".repeat(20), to: "0x" + "22".repeat(20), nonce: "0x01", valid_before: 9999999999 };

test("T-S1 DRY_RUN prints the plan, exits 0, reads no secret and writes nothing", () => {
  const { r, out, written } = relay({ DRY_RUN: "1", BASE_RELAYER_KEY: KEY, BASE_SEPOLIA_RPC: `https://sepolia.base.org/${PATHKEY}` });
  assert.equal(r.status, 0, out);
  assert.match(out, /DRY RUN/);
  assert.ok(!out.includes(KEY) && !out.includes(PATHKEY));
  assert.equal(written, "");
});

test("T-S2 a network other than testnet is refused", () => {
  const { r, out } = relay({ AGENT_EXIT_NETWORK: "mainnet", BASE_RELAYER_KEY: KEY }, { samples: [{ ok: true, auth: GOOD_AUTH }] });
  assert.equal(r.status, 1);
  assert.match(out, /not testnet/);
});

test("T-S3 a non-allowlisted Base RPC is refused and its path (an API key) is never printed", () => {
  for (const rpc of [`https://evil.example/${PATHKEY}`, `http://sepolia.base.org/${PATHKEY}`, `https://mainnet.base.org/${PATHKEY}`, `https://user:${PATHKEY}@sepolia.base.org`, `not a url ${PATHKEY}`]) {
    const { r, out, written } = relay({ BASE_RELAYER_KEY: KEY, BASE_SEPOLIA_RPC: rpc }, { samples: [{ ok: true, auth: GOOD_AUTH }] });
    assert.equal(r.status, 1, rpc);
    assert.match(out, /refusing Base RPC/);
    assert.ok(!out.includes(PATHKEY) && !out.includes(KEY), `leak for ${rpc}: ${out}`);
    assert.ok(!written.includes(PATHKEY) && !written.includes(KEY));
  }
});

test("T-S4 the relayer key is checked without being echoed", () => {
  const bad = "0xNOT-A-KEY-but-secret-looking-xyz";
  const { r, out, written } = relay({ BASE_RELAYER_KEY: bad }, { samples: [{ ok: true, auth: GOOD_AUTH }] });
  assert.equal(r.status, 1);
  assert.ok(!out.includes(bad) && !written.includes(bad));
  assert.equal(relay({}, { samples: [] }).r.status, 1);
  assert.throws(() => checkRelayerKey(""), /missing/);
  assert.equal(checkRelayerKey("ab".repeat(32)), "0x" + "ab".repeat(32));
});

test("T-S5 only a small transferWithAuthorization is relayed", () => {
  assert.ok(checkAuthorization(GOOD_AUTH));
  assert.throws(() => checkAuthorization({ ...GOOD_AUTH, calldata: "0xa9059cbb" + "00".repeat(64) }), /not transferWithAuthorization/);
  assert.throws(() => checkAuthorization({ ...GOOD_AUTH, value: "100000001" }), /outside/);
  assert.throws(() => checkAuthorization({ ...GOOD_AUTH, value: "0" }), /outside/);
  assert.throws(() => checkAuthorization({ ...GOOD_AUTH, value: "abc" }), /not a number/);
  const { r, out } = relay({ BASE_RELAYER_KEY: KEY }, { samples: [{ ok: true, auth: { ...GOOD_AUTH, calldata: "0xdeadbeef" } }] });
  assert.equal(r.status, 1);
  assert.match(out, /not transferWithAuthorization/);
});

test("T-S3 guards: hosts, env parsing and scrubbing", () => {
  assert.equal(checkBaseRpc("https://sepolia.base.org"), "https://sepolia.base.org");
  assert.throws(() => checkBaseRpc("https://sepolia.base.org:8545"), /refusing/);
  assert.deepEqual(parseEnvFile("# c\nexport A='1'\nB=2\n"), { A: "1", B: "2" });
  assert.equal(scrub(`k=${KEY} p=${PATHKEY}`, [KEY, PATHKEY, undefined, "short"]), "k=[redacted] p=[redacted]");
});
