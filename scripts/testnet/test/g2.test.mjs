// Safety rules of the G2 relay (scenario ids T-S1..T-S5): testnet only,
// secrets never echoed, DRY_RUN sends nothing. Offline; viem is not needed
// because every refusal happens before it is imported.
import { test } from "node:test";
import assert from "node:assert/strict";
import { spawnSync } from "node:child_process";
import { chmodSync, mkdtempSync, readFileSync, writeFileSync, existsSync } from "node:fs";
import { tmpdir } from "node:os";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";
import { checkAuthorization, checkBaseRpc, checkChainsig, checkRelayerKey, checkSecretFileMode, parseEnvFile, requireTestnet, scrub } from "../g2-guards.mjs";

const RELAY = join(dirname(fileURLToPath(import.meta.url)), "..", "g2-relay.mjs");
const KEY = "0x" + "ab12".repeat(16);
const PATHKEY = "super-secret-rpc-token-123456";

function relay(env, signJson) {
  const dir = mkdtempSync(join(tmpdir(), "g2-"));
  const sign = join(dir, "g2-sign.json");
  if (signJson) writeFileSync(sign, JSON.stringify(signJson));
  const base = { PATH: process.env.PATH, AGENT_EXIT_ENV: "/nonexistent", AGENT_EXIT_NETWORK: "testnet" };
  const r = spawnSync(process.execPath, [RELAY, sign], { env: { ...base, ...env }, encoding: "utf8" });
  const outFile = join(dir, "g2.json");
  return { r, out: r.stdout + r.stderr, written: existsSync(outFile) ? readFileSync(outFile, "utf8") : "" };
}
const word = (hex) => hex.replace(/^0x/, "").padStart(64, "0");
/** transferWithAuthorization(from,to,value,validAfter,validBefore,nonce,v,r,s) */
const calldata = ({ from, to, value, nonce }) =>
  "0xe3ee160e" + [from, to, BigInt(value).toString(16), "0", "ffffffff", nonce, "1b", "aa".repeat(32), "bb".repeat(32)].map(word).join("");
const FIELDS = { from: "0x" + "11".repeat(20), to: "0x" + "22".repeat(20), value: "1000000", nonce: "0x01" };
const GOOD_AUTH = { ...FIELDS, calldata: calldata(FIELDS), valid_before: 9999999999 };

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

test("T-S6 the value inside the calldata must equal auth.value (and from, to, nonce)", () => {
  assert.ok(checkAuthorization(GOOD_AUTH));
  // JSON says 1 USDC, the calldata moves 99 USDC: refused
  const lie = { ...GOOD_AUTH, calldata: calldata({ ...FIELDS, value: "99000000" }) };
  assert.throws(() => checkAuthorization(lie), /calldata value 99000000 != auth.value 1000000/);
  // a calldata value above the cap is refused even when the JSON field agrees
  const big = { ...GOOD_AUTH, value: "100000001", calldata: calldata({ ...FIELDS, value: "100000001" }) };
  assert.throws(() => checkAuthorization(big), /outside/);
  assert.throws(() => checkAuthorization({ ...GOOD_AUTH, calldata: calldata({ ...FIELDS, to: "0x" + "33".repeat(20) }) }), /calldata to/);
  assert.throws(() => checkAuthorization({ ...GOOD_AUTH, calldata: calldata({ ...FIELDS, from: "0x" + "44".repeat(20) }) }), /calldata from/);
  assert.throws(() => checkAuthorization({ ...GOOD_AUTH, calldata: calldata({ ...FIELDS, nonce: "0x02" }) }), /calldata nonce/);
  assert.throws(() => checkAuthorization({ ...GOOD_AUTH, calldata: GOOD_AUTH.calldata.slice(0, -2) }), /length/);
  const { r, out } = relay({ BASE_RELAYER_KEY: KEY }, { samples: [{ ok: true, auth: lie }] });
  assert.equal(r.status, 1);
  assert.match(out, /calldata value/);
});

test("T-S7 AGENT_EXIT_NETWORK must be set, and must be testnet", () => {
  const sign = { samples: [{ ok: true, auth: GOOD_AUTH }] };
  const { r, out } = relay({ BASE_RELAYER_KEY: KEY, AGENT_EXIT_NETWORK: "" }, sign);
  assert.equal(r.status, 1);
  assert.match(out, /AGENT_EXIT_NETWORK is not set/);
  assert.throws(() => requireTestnet({}), /not set/);
  assert.throws(() => requireTestnet({ AGENT_EXIT_NETWORK: "offline" }), /not testnet/);
  assert.doesNotThrow(() => requireTestnet({ AGENT_EXIT_NETWORK: "testnet" }));
});

test("T-S8 a chainsig.js address that differs, or cannot be derived, stops the relay", () => {
  assert.doesNotThrow(() => checkChainsig({ chainsig_js_address: "0xab", chainsig_js_matches: true }));
  assert.throws(() => checkChainsig({ chainsig_js_address: "0xab", chainsig_js_matches: false }), /does not match the contract address/);
  assert.throws(() => checkChainsig({ chainsig_js_error: "boom" }), /could not derive/);
  assert.throws(() => checkChainsig({}), /could not derive/);
});

test("T-S9 secret files must be mode 600: the env file is refused when group- or world-readable", () => {
  const dir = mkdtempSync(join(tmpdir(), "g2m-"));
  const f = join(dir, "env");
  writeFileSync(f, `BASE_RELAYER_KEY=${KEY}\n`);
  for (const mode of [0o644, 0o640, 0o604, 0o660, 0o666]) {
    chmodSync(f, mode);
    assert.throws(() => checkSecretFileMode(f), /chmod 600/, mode.toString(8));
  }
  for (const mode of [0o600, 0o400]) {
    chmodSync(f, mode);
    assert.doesNotThrow(() => checkSecretFileMode(f));
  }
  assert.doesNotThrow(() => checkSecretFileMode(join(dir, "missing")));
  chmodSync(f, 0o644);
  const { r, out } = relay({ AGENT_EXIT_ENV: f }, { samples: [{ ok: true, auth: GOOD_AUTH }] });
  assert.equal(r.status, 1);
  assert.match(out, /chmod 600/);
  assert.ok(!out.includes(KEY));
  chmodSync(f, 0o600);
  const ok = relay({ AGENT_EXIT_ENV: f, BASE_SEPOLIA_RPC: "https://evil.example/x" }, { samples: [{ ok: true, auth: GOOD_AUTH }] });
  assert.match(ok.out, /refusing Base RPC/); // got past the mode check
});

test("T-S3 guards: hosts, env parsing and scrubbing", () => {
  assert.equal(checkBaseRpc("https://sepolia.base.org"), "https://sepolia.base.org");
  assert.throws(() => checkBaseRpc("https://sepolia.base.org:8545"), /refusing/);
  assert.deepEqual(parseEnvFile("# c\nexport A='1'\nB=2\n"), { A: "1", B: "2" });
  assert.equal(scrub(`k=${KEY} p=${PATHKEY}`, [KEY, PATHKEY, undefined, "short"]), "k=[redacted] p=[redacted]");
});
