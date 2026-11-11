#!/usr/bin/env node
// Signs, by script, the exact transaction the page's "Invest" button builds
// (web/wallet-link.mjs investTx + encodeTx) for an outside testnet account, and
// records it in out/invest-button-testnet.json. The wallet fills in three
// placeholders (public key, nonce, block hash); this script fills in the same
// three and checks that every other byte equals the page's encoding.
// Testnet only. The key is read from ~/.agent-exit/keys/<name>.key and never printed.
//   node scripts/testnet/invest-button.mjs p6.agentexit5.testnet
import { createHash } from "node:crypto";
import { readFileSync, statSync, writeFileSync } from "node:fs";
import { homedir } from "node:os";
import { join } from "node:path";
import { encodeTx, investTx } from "../../web/wallet-link.mjs";
import { NearKey, b58decode, b58encode } from "../../mcp/dist/src/nearsign.js";

const RPC = process.env.AGENT_EXIT_RPC || "https://rpc.testnet.fastnear.com";
const account = process.argv[2];
if (!/^[a-z0-9_-]+\.[a-z0-9._-]+\.testnet$/.test(account ?? "")) {
  console.error("usage: invest-button.mjs <sub-account>.testnet (testnet only)");
  process.exit(2);
}
const keyFile = join(homedir(), ".agent-exit", "keys", `${account.split(".")[0]}.key`);
if ((statSync(keyFile).mode & 0o077) !== 0) { console.error(`${keyFile} must be mode 600`); process.exit(2); }
const key = new NearKey(readFileSync(keyFile, "utf8"));

const rpc = async (method, params) => {
  const r = await (await fetch(RPC, { method: "POST", headers: { "content-type": "application/json" }, body: JSON.stringify({ jsonrpc: "2.0", id: 1, method, params }) })).json();
  if (r.error) throw new Error(`${method}: ${JSON.stringify(r.error).slice(0, 300)}`);
  return r.result;
};
const view = async (acct, m, args) => {
  const r = await rpc("query", { request_type: "call_function", finality: "final", account_id: acct, method_name: m, args_base64: Buffer.from(JSON.stringify(args)).toString("base64") });
  return JSON.parse(Buffer.from(r.result).toString());
};

const o = JSON.parse(readFileSync("web/data/testnet.json", "utf8")).open_offer;
if (!o?.agent?.endsWith(".testnet") || !o?.xusd?.endsWith(".testnet")) throw new Error("web/data/testnet.json has no testnet open_offer");
const bps = (h) => (h.holdings ?? []).reduce((n, x) => n + Number(x.bps), 0);
const before = await view(o.agent, "get_holdings", { account_id: account });

// The page's bytes, with the wallet's three placeholders.
const page = encodeTx(investTx(o, account));
const signerLen = 4 + Buffer.byteLength(account);
const pkAt = signerLen + 1;               // after the key-type tag (0 = ed25519)
const nonceAt = pkAt + 32;
const recvAt = nonceAt + 8;
const hashAt = recvAt + 4 + Buffer.byteLength(o.xusd);

const ak = await rpc("query", { request_type: "view_access_key", finality: "final", account_id: account, public_key: key.publicKeyString });
const block = await rpc("block", { finality: "final" });
const tx = Buffer.from(page);
key.publicKey.copy(tx, pkAt);
tx.writeBigUInt64LE(BigInt(ak.nonce) + 1n, nonceAt);
Buffer.from(b58decode(block.header.hash)).copy(tx, hashAt);

// Every byte outside the three placeholders equals the page's encoding.
const same = (a, b) => Buffer.compare(Buffer.from(a), Buffer.from(b)) === 0;
const ok = same(tx.subarray(0, pkAt), page.subarray(0, pkAt))
  && same(tx.subarray(nonceAt + 8, hashAt), page.subarray(nonceAt + 8, hashAt))
  && same(tx.subarray(hashAt + 32), page.subarray(hashAt + 32));
if (!ok) throw new Error("the signed bytes differ from the page's transaction outside the placeholders");

const digest = createHash("sha256").update(tx).digest();
const signed = Buffer.concat([tx, Buffer.from([0]), key.sign(digest)]);
const hash = b58encode(digest);
const res = await rpc("send_tx", { signed_tx_base64: signed.toString("base64"), wait_until: "FINAL" });
const success = res.status && "SuccessValue" in res.status;
const after = await view(o.agent, "get_holdings", { account_id: account });

const out = {
  schema: "invest-button-testnet.v1",
  money: "xUSD-FAKE (test token, no value)",
  note: "the same transaction the page's Invest button builds (web/wallet-link.mjs), signed by script",
  account,
  agent: o.agent,
  token: o.xusd,
  actions: investTx(o, account).actions.map((a) => ({ method: a.method, gas_tgas: a.gasTgas, deposit_yocto: String(a.deposit) })),
  bytes_equal_to_page_except_key_nonce_blockhash: ok,
  tx,
  tx_hash: hash,
  explorer: `https://testnet.nearblocks.io/txns/${hash}`,
  status_ok: Boolean(success),
  holdings_bps_before: bps(before),
  holdings_bps_after: bps(after),
  pass: Boolean(success) && bps(after) - bps(before) === Number(o.part_bps),
};
delete out.tx;
writeFileSync("out/invest-button-testnet.json", JSON.stringify(out, null, 2) + "\n");
console.log(`invest-button: ${account} ${out.pass ? "PASS" : "FAIL"}: ${bps(before) / 100}% -> ${bps(after) / 100}% of ${o.agent}; tx ${hash}`);
process.exit(out.pass ? 0 : 1);
