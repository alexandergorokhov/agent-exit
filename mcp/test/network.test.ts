// Never mainnet; testnet reads need an env var, testnet writes also need a key in the server env.
import { test } from "node:test";
import assert from "node:assert/strict";
import { spawnSync } from "node:child_process";
import { SERVER } from "../src/client.js";
import { assertNotMainnet, loadConfig } from "../src/config.js";
import { TestnetBackend, wasmExports } from "../src/backend.js";
import { b58decode, b58encode, NearKey, signTx } from "../src/nearsign.js";

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

test("testnet buy without a key or token sends nothing; list is not built", async () => {
  const cfg = loadConfig({ AGENT_EXIT_NETWORK: "testnet", AGENT_EXIT_TOKEN: "xusd.agentexit3.testnet" });
  let calls = 0;
  const f = (async () => {
    calls++;
    throw new Error("no network");
  }) as unknown as typeof fetch;
  const noKey = new TestnetBackend(cfg, f, null);
  await assert.rejects(noKey.buy("translator.agentexit3.testnet", "buyer.agentexit3.testnet", "ed25519:x", "1", 1, "h"), (e: { code: string }) => e.code === "WRITE_DISABLED");
  const noToken = new TestnetBackend(loadConfig({ AGENT_EXIT_NETWORK: "testnet" }), f, NS_SECRET);
  await assert.rejects(noToken.buy("translator.agentexit3.testnet", "buyer.agentexit3.testnet", "ed25519:x", "1", 1, "h"), (e: { code: string }) => e.code === "WRITE_DISABLED");
  const mainnet = new TestnetBackend(cfg, f, NS_SECRET);
  await assert.rejects(mainnet.buy("translator.near", "buyer.agentexit3.testnet", "ed25519:x", "1", 1, "h"), (e: { code: string }) => e.code === "NETWORK_REFUSED");
  assert.equal(calls, 0, "nothing was sent");
  await assert.rejects(noKey.list(), (e: { code: string }) => e.code === "NOT_BUILT");
});

// NS1: the hand-written borsh and signature equal near-api-js 5.1.1 byte for
// byte (vector made once with near-api-js; TEST key, seed = 32 x 0x09).
const NS_SECRET = "ed25519:BUguQsv2ZuHus54HAFzjdJHzZBkygAjKhEeYwSG19tUwuftFhkkkEFDzcow9XDkfD3qGi1Yb3LV15ufdrKJ226T";
const NS_PK = "ed25519:J2xccRtuG43drESLYznHhLhQkLTdfepcKYbiQ9BsJVaf";
const NS_SIGNED = "1800000062757965722e6167656e7465786974332e746573746e657400fd1724385aa0c75b64fb78cd602fa1d991fdebf76b13c58ed702eac835e9f6182a0000000000000017000000787573642e6167656e7465786974332e746573746e6574070707070707070707070707070707070707070707070707070707070707070701000000021000000066745f7472616e736665725f63616c6c500000007b2272656365697665725f6964223a227472616e736c61746f722e6167656e7465786974332e746573746e6574222c22616d6f756e74223a2231303030303030303030222c226d7367223a227b7d227d00c06e31d910010001000000000000000000000000000000005c56c5bf7679874f39cf6f32759495b41cec3f8b26fd4b575daf49cda4d4cd9aacc8fc96ee2715d26fc22ed754625948c16de8f0bff633ef9d58d26cf3851203";

test("NS1 transaction signing matches near-api-js", () => {
  const k = new NearKey(NS_SECRET);
  assert.equal(k.publicKeyString, NS_PK);
  const { signed } = signTx(
    {
      signerId: "buyer.agentexit3.testnet",
      publicKey: k.publicKey,
      nonce: 42n,
      receiverId: "xusd.agentexit3.testnet",
      blockHash: Buffer.alloc(32, 7),
      methodName: "ft_transfer_call",
      args: Buffer.from('{"receiver_id":"translator.agentexit3.testnet","amount":"1000000000","msg":"{}"}'),
      gas: 300_000_000_000_000n,
      deposit: 1n,
    },
    k,
  );
  assert.equal(signed.toString("hex"), NS_SIGNED);
  assert.equal(b58encode(b58decode(NS_PK.slice(8))), NS_PK.slice(8));
});

test("NS2 testnet buy: signs one ft_transfer_call with the scanned version and hash, reads the sale event", async () => {
  const cfg = loadConfig({ AGENT_EXIT_NETWORK: "testnet", AGENT_EXIT_TOKEN: "xusd.agentexit3.testnet", AGENT_EXIT_SIGNER_PK: NS_PK });
  const sent: { method: string; params: Record<string, unknown> }[] = [];
  const ev = (event: string, data: unknown) => `EVENT_JSON:${JSON.stringify({ standard: "agent_exit", version: "1.0.0", event, data: [data] })}`;
  const sale = { sale_id: 3, seller: "summarizer.agentexit3.testnet", buyer: "buyer.agentexit3.testnet", price: "1000000000", paid_now: "700000000", escrow: "300000000", overpay_refund: "0" };
  const f = (async (_u: string, init: RequestInit) => {
    const b = JSON.parse(String(init.body));
    sent.push(b);
    const result =
      b.method === "query"
        ? { nonce: 41, block_hash: "11111111111111111111111111111111", permission: "FullAccess" }
        : { status: { SuccessValue: "" }, receipts_outcome: [{ outcome: { logs: [ev("settling", { sale_id: 3, delete_keys: ["ed25519:old"], add_key: "ed25519:new" }), ev("sale", sale)] } }] };
    return new Response(JSON.stringify({ jsonrpc: "2.0", id: b.id, result }), { status: 200 });
  }) as unknown as typeof fetch;
  const b = new TestnetBackend(cfg, f, NS_SECRET);
  const out = await b.buy("translator.agentexit3.testnet", "buyer.agentexit3.testnet", "ed25519:new", "1000000000", 2, "khash");
  assert.equal(out.sale.sale_id, 3);
  assert.deepEqual(out.key_batch, [{ action: "delete", key: "ed25519:old" }, { action: "add_fc", key: "ed25519:new" }]);
  assert.match(out.explorer ?? "", /^https:\/\/testnet\.nearblocks\.io\/txns\/[1-9A-HJ-NP-Za-km-z]{43,44}$/);
  assert.equal(sent.length, 2);
  assert.equal(sent[1].method, "send_tx");
  // Decode the signed bytes: receiver is the token, args name the scanned version and hash.
  const raw = Buffer.from(String(sent[1].params.signed_tx_base64), "base64").toString("latin1");
  assert.ok(raw.includes("xusd.agentexit3.testnet") && raw.includes("ft_transfer_call"));
  assert.ok(raw.includes('\\"listing_version\\":2') && raw.includes('\\"known_keys_hash\\":\\"khash\\"'), raw);
  // No sale event: refused, refunded, said plainly.
  const refused = (async (_u: string, init: RequestInit) => {
    const q = JSON.parse(String(init.body));
    const result = q.method === "query" ? { nonce: 1, block_hash: "11111111111111111111111111111111" } : { status: { SuccessValue: "" }, receipts_outcome: [{ outcome: { logs: [] } }] };
    return new Response(JSON.stringify({ jsonrpc: "2.0", id: q.id, result }), { status: 200 });
  }) as unknown as typeof fetch;
  await assert.rejects(new TestnetBackend(cfg, refused, NS_SECRET).buy("translator.agentexit3.testnet", "buyer.agentexit3.testnet", "ed25519:new", "1", 2, "h"), (e: { code: string }) => e.code === "BUY_REFUSED");
  // Send fails: may have been sent, never "not sent".
  const lost = (async (_u: string, init: RequestInit) => {
    const q = JSON.parse(String(init.body));
    if (q.method !== "query") throw new Error("timeout");
    return new Response(JSON.stringify({ jsonrpc: "2.0", id: q.id, result: { nonce: 1, block_hash: "11111111111111111111111111111111" } }), { status: 200 });
  }) as unknown as typeof fetch;
  await assert.rejects(new TestnetBackend(cfg, lost, NS_SECRET).buy("translator.agentexit3.testnet", "buyer.agentexit3.testnet", "ed25519:new", "1", 2, "h"), (e: { code: string }) => e.code === "SENT_UNCONFIRMED");
  // A signer key that is not AGENT_EXIT_SIGNER_PK is refused before any call.
  const other = loadConfig({ AGENT_EXIT_NETWORK: "testnet", AGENT_EXIT_TOKEN: "xusd.agentexit3.testnet", AGENT_EXIT_SIGNER_PK: "ed25519:J2xccRtuG43drESLYznHhLhQkLTdfepcKYbiQ9BsJVaa" });
  await assert.rejects(new TestnetBackend(other, f, NS_SECRET).buy("translator.agentexit3.testnet", "buyer.agentexit3.testnet", "ed25519:new", "1", 2, "h"), (e: { code: string }) => e.code === "WRITE_DISABLED");
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
