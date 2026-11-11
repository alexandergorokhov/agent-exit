// Checks that every transaction the page links (web/data/testnet.json) is
// known to NEAR testnet: one `tx` query per hash on an archival node.
// Needs network access to NEAR RPC (run it on the machine that ran the scenes).
import { readFileSync } from "node:fs";

const RPC = process.env.AGENT_EXIT_ARCHIVAL_RPC || "https://archival-rpc.testnet.fastnear.com";
const data = JSON.parse(readFileSync("web/data/testnet.json", "utf8"));
// A rate-limited answer (HTTP 429 or JSON-RPC code -429) is not "not found":
// wait and ask again (2, 4, 8, 16, 32 s), and pace the queries.
const sleep = (ms) => new Promise((r) => setTimeout(r, ms));
async function lookup(t) {
  let r;
  for (let attempt = 0; attempt < 6; attempt++) {
    r = await fetch(RPC, {
      method: "POST",
      headers: { "content-type": "application/json" },
      body: JSON.stringify({ jsonrpc: "2.0", id: "c", method: "tx", params: { tx_hash: t.tx, sender_account_id: t.by, wait_until: "NONE" } }),
    }).then(async (x) => (x.status === 429 ? { error: { code: -429, message: "HTTP 429" } } : x.json())).catch((e) => ({ error: { message: String(e) } }));
    if (!(r.error && (r.error.code === -429 || /rate limit/i.test(String(r.error.message))))) break;
    await sleep(2000 * 2 ** attempt);
  }
  await sleep(250);
  return r;
}
let ok = 0;
const bad = [];
for (const [step, s] of Object.entries(data.steps)) {
  for (const t of s.txs) {
    if (!t.tx) continue;
    const r = await lookup(t);
    if (r.result && r.result.transaction && r.result.transaction.hash === t.tx) ok++;
    else bad.push(`${step}: ${t.tx} (${t.label}): ${JSON.stringify(r.error || "hash mismatch").slice(0, 120)}`);
  }
}
// The Invest button's recorded transaction (page data open_offer.example_tx).
const ex = data.open_offer && data.open_offer.example_tx;
if (ex && ex.tx) {
  const r = await lookup({ tx: ex.tx, by: ex.account });
  if (r.result && r.result.transaction && r.result.transaction.hash === ex.tx) ok++;
  else bad.push(`open_offer.example_tx: ${ex.tx}: ${JSON.stringify(r.error || "hash mismatch").slice(0, 120)}`);
}
console.log(`check-tx-links: ${ok} found, ${bad.length} not found`);
for (const b of bad) console.log(`  ${b}`);
process.exit(bad.length ? 1 : 0);
