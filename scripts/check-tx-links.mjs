// Checks that every transaction the page links (web/data/testnet.json) is
// known to NEAR testnet: one `tx` query per hash on an archival node.
// Needs network access to NEAR RPC (run it on the machine that ran the scenes).
import { readFileSync } from "node:fs";

const RPC = process.env.AGENT_EXIT_ARCHIVAL_RPC || "https://archival-rpc.testnet.fastnear.com";
const data = JSON.parse(readFileSync("web/data/testnet.json", "utf8"));
let ok = 0;
const bad = [];
for (const [step, s] of Object.entries(data.steps)) {
  for (const t of s.txs) {
    if (!t.tx) continue;
    const r = await fetch(RPC, {
      method: "POST",
      headers: { "content-type": "application/json" },
      body: JSON.stringify({ jsonrpc: "2.0", id: "c", method: "tx", params: { tx_hash: t.tx, sender_account_id: t.by, wait_until: "NONE" } }),
    }).then((x) => x.json()).catch((e) => ({ error: { message: String(e) } }));
    if (r.result && r.result.transaction && r.result.transaction.hash === t.tx) ok++;
    else bad.push(`${step}: ${t.tx} (${t.label}): ${JSON.stringify(r.error || "hash mismatch").slice(0, 120)}`);
  }
}
console.log(`check-tx-links: ${ok} found, ${bad.length} not found`);
for (const b of bad) console.log(`  ${b}`);
process.exit(bad.length ? 1 : 0);
