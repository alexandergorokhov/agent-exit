// A51: the scan checks the agent's Base address against the MPC derivation, so
// the MCP server must hand the scanner contract.base_wallet and the MPC root
// key. Scenario ids: BW1 testnet input, BW2 through the real scanner, BW3 the
// two providers must agree, BW4 only the published signer is asked, BW5 a
// missing view blocks instead of crashing, BW6 offline fixtures.
import { test } from "node:test";
import assert from "node:assert/strict";
import { TestnetBackend } from "../src/backend.js";
import { loadConfig } from "../src/config.js";
import { loadSchemas, Tools } from "../src/tools.js";
import { REPO } from "./helpers.js";
import { connect } from "../src/client.js";

const AGENT = "translator.testnet";
const MPC = "v1.signer-prod.testnet";
// The address and MPC root key the offline fixture world uses for the translator.
const ADDR = "0xf1e4aa61e3f624e873f4f92217dcee594e092544";
const MPC_PK = "secp256k1:jCrfJ5wjMBroDh6Mefb1bohaBtzE9me5MaKEhmfmFqK5CudvdSWviXXxfowdtttqh9KWN3zNK9QQu5kq8FfXBK8";
const RPC1 = "https://rpc.testnet.near.org";
const RPC2 = "https://test.rpc.fastnear.com";

interface Opts {
  address?: string | null;
  mpcPk?: Record<string, string | undefined>; // per provider url
  mpcField?: string;
  noBaseView?: boolean;
}
/** A fake testnet: the agent's views, access keys, code, and the MPC signer. */
function fakeChain(o: Opts = {}) {
  const asked: { url: string; account: string; method: string }[] = [];
  const f = (async (url: string, init?: RequestInit) => {
    const p = JSON.parse(String(init?.body)).params;
    const view = (v: unknown) => ({ result: { result: [...Buffer.from(JSON.stringify(v))] } });
    let result: unknown;
    if (p.request_type === "call_function") {
      asked.push({ url, account: p.account_id, method: p.method_name });
      if (p.account_id === MPC && p.method_name === "public_key") {
        const pk = o.mpcPk ? o.mpcPk[url] : MPC_PK;
        if (pk === undefined) return new Response(JSON.stringify({ result: { error: "no such method" } }), { status: 200 });
        result = view(pk).result;
      } else if (p.account_id === AGENT) {
        switch (p.method_name) {
          case "get_status":
            result = view({ status: "Listed", owner: "seller.testnet", listing_version: 1, known_keys: [], known_keys_hash: "00", open_approvals: 0, frozen: false, admin: "admin.testnet", token: "xusd.testnet", paid_to_total: 0, near_storage: { solvent: true } }).result;
            break;
          case "get_listing":
            result = view(null).result;
            break;
          case "get_base_wallet":
            if (o.noBaseView) return new Response(JSON.stringify({ result: { error: "MethodNotFound" } }), { status: 200 });
            result = view({ schema: "base-wallet.v1", address: o.address === undefined ? ADDR : o.address, path: "wallet-1", mpc: o.mpcField ?? MPC }).result;
            break;
          default:
            return new Response(JSON.stringify({ result: { error: `unexpected ${p.method_name}` } }), { status: 200 });
        }
      } else {
        return new Response(JSON.stringify({ result: { error: `unexpected account ${p.account_id}` } }), { status: 200 });
      }
    } else if (p.request_type === "view_access_key_list") result = { keys: [] };
    else if (p.request_type === "view_account") result = { code_hash: "11111111111111111111111111111111" };
    else result = { code_base64: Buffer.from([0, 0x61, 0x73, 0x6d, 1, 0, 0, 0]).toString("base64") };
    return new Response(JSON.stringify({ result }), { status: 200 });
  }) as unknown as typeof fetch;
  return { f, asked };
}
const cfg = () => loadConfig({ AGENT_EXIT_NETWORK: "testnet" });
const backend = (o?: Opts) => {
  const c = fakeChain(o);
  return { b: new TestnetBackend(cfg(), c.f), asked: c.asked };
};
const codes = (r: { body: { findings: { code: string }[] } }) => r.body.findings.map((x) => x.code);
const scan = (b: TestnetBackend) => new Tools(cfg(), b, loadSchemas(REPO)).call("scan_agent", { agent: AGENT });

test("BW1 testnet scan input carries get_base_wallet and the MPC root key", async () => {
  const { b, asked } = backend();
  const input = (await b.scanInput(AGENT, {})) as { contract: { base_wallet: Record<string, unknown> } };
  assert.deepEqual(input.contract.base_wallet, { address: ADDR, path: "wallet-1", mpc_public_key: MPC_PK });
  // the key comes from the signer contract itself, from both providers
  const mpcReads = asked.filter((a) => a.account === MPC);
  assert.deepEqual(mpcReads.map((a) => a.url).sort(), [RPC1, RPC2].sort());
  assert.ok(asked.some((a) => a.account === AGENT && a.method === "get_base_wallet"));
  // an agent with no Base wallet passes address null through
  const none = (await backend({ address: null }).b.scanInput(AGENT, {})) as { contract: { base_wallet: { address: unknown } } };
  assert.equal(none.contract.base_wallet.address, null);
});

test("BW2 the real scanner accepts the right address and blocks a different one", async () => {
  const ok = await scan(backend().b);
  assert.ok(ok.ok, JSON.stringify(ok.body));
  assert.ok(!codes(ok).some((c) => c === "base_wallet_unchecked" || c === "base_address_mismatch"), codes(ok).join(","));
  const evil = await scan(backend({ address: "0x" + "ab".repeat(20) }).b);
  assert.ok(codes(evil).includes("base_address_mismatch"), codes(evil).join(","));
  assert.equal(evil.body.safe_to_buy, false);
});

test("BW3 the two RPC providers must agree on the MPC key; otherwise the scan blocks as unchecked", async () => {
  const split = await scan(backend({ mpcPk: { [RPC1]: MPC_PK, [RPC2]: "secp256k1:" + "1".repeat(80) } }).b);
  assert.ok(codes(split).includes("base_wallet_unchecked"), codes(split).join(","));
  assert.equal(split.body.safe_to_buy, false);
  const one = await scan(backend({ mpcPk: { [RPC1]: MPC_PK, [RPC2]: undefined } }).b);
  assert.ok(codes(one).includes("base_wallet_unchecked"));
});

test("BW4 only the published MPC signer is asked; the agent cannot name another", async () => {
  const { b, asked } = backend({ mpcField: "attacker.testnet" });
  const input = (await b.scanInput(AGENT, {})) as { contract: { base_wallet: { mpc_public_key: unknown } } };
  assert.equal(input.contract.base_wallet.mpc_public_key, null);
  assert.ok(asked.every((a) => a.account === AGENT), "no read went to the account the agent named");
  const r = await scan(backend({ mpcField: "attacker.testnet" }).b);
  assert.ok(codes(r).includes("base_wallet_unchecked"));
});

test("BW5 an agent without get_base_wallet is blocked as unchecked, not a crash", async () => {
  const input = (await backend({ noBaseView: true }).b.scanInput(AGENT, {})) as { contract: Record<string, unknown> };
  assert.ok(!("base_wallet" in input.contract) || input.contract.base_wallet === undefined);
  const r = await scan(backend({ noBaseView: true }).b);
  assert.ok(codes(r).includes("base_wallet_unchecked"), codes(r).join(","));
});

test("BW6 offline fixtures: the translator scans clean of Base findings; cheat is still blocked", async () => {
  const s = await connect({});
  try {
    const t = await s.call("scan_agent", { agent: AGENT });
    assert.ok(!codes(t).some((c) => c.startsWith("base_")), codes(t).join(","));
    assert.equal(t.body.safe_to_buy, true);
    const c = await s.call("scan_agent", { agent: "cheat.testnet" });
    assert.equal(c.body.safe_to_buy, false);
  } finally {
    await s.close();
  }
});

test("RL1 a rate-limited RPC (HTTP 429) is retried with backoff, then answers", async () => {
  const { RETRY_429_MS } = await import("../src/backend.js");
  const saved = [...RETRY_429_MS];
  RETRY_429_MS.splice(0, 2, 1, 1);
  try {
    const c = fakeChain();
    let calls = 0;
    const f = (async (url: string, init?: RequestInit) => {
      calls++;
      if (calls <= 2) return new Response("Too Many Requests", { status: 429 });
      return (c.f as unknown as (u: string, i?: RequestInit) => Promise<Response>)(url, init);
    }) as unknown as typeof fetch;
    const b = new TestnetBackend(cfg(), f) as unknown as { rpc(u: string, p: unknown): Promise<{ keys: unknown[] }> };
    const r = await b.rpc(RPC1, { request_type: "view_access_key_list", account_id: AGENT, finality: "final" });
    assert.deepEqual(r.keys, []);
    assert.equal(calls, 3, "two 429s, then the answer");
  } finally {
    RETRY_429_MS.splice(0, 2, ...saved);
  }
});

test("RL2 a provider that keeps answering 429 gives RPC_ERROR after two retries", async () => {
  const { RETRY_429_MS } = await import("../src/backend.js");
  const saved = [...RETRY_429_MS];
  RETRY_429_MS.splice(0, 2, 1, 1);
  try {
    let calls = 0;
    const f = (async () => {
      calls++;
      return new Response("Too Many Requests", { status: 429 });
    }) as unknown as typeof fetch;
    const b = new TestnetBackend(cfg(), f) as unknown as { rpc(u: string, p: unknown): Promise<unknown> };
    await assert.rejects(() => b.rpc(RPC1, { request_type: "view_access_key_list", account_id: AGENT, finality: "final" }), /HTTP 429/);
    assert.equal(calls, 3, "one try and two retries");
  } finally {
    RETRY_429_MS.splice(0, 2, ...saved);
  }
});
