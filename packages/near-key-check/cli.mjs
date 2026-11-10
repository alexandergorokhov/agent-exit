#!/usr/bin/env node
// near-key-check <account.testnet> [--published <code_hash>] [--json]
// Exit 0 locked, 1 not locked, 2 error. Read-only.
import { checkAccount } from "./index.mjs";

const args = process.argv.slice(2);
const account = args.find((a) => !a.startsWith("--") && args[args.indexOf(a) - 1] !== "--published");
const pi = args.indexOf("--published");
const publishedHash = pi >= 0 ? args[pi + 1] : process.env.AGENT_EXIT_CODE_HASH;
if (!account) {
  console.error("usage: near-key-check <account.testnet> [--published <code_hash>] [--json]");
  process.exit(2);
}
try {
  const r = await checkAccount(account.toLowerCase(), { publishedHash });
  if (args.includes("--json")) console.log(JSON.stringify(r, null, 2));
  else {
    console.log(`${r.account}: ${r.verdict}\n${r.summary}`);
    for (const f of r.findings) console.log(`  [${f.severity}] ${f.code}: ${f.text}`);
    for (const k of r.keys) console.log(`  key ${k.public_key}: ${k.can}`);
  }
  process.exit(r.verdict === "locked" ? 0 : 1);
} catch (e) {
  console.error(`near-key-check: ${e.message}`);
  process.exit(2);
}
