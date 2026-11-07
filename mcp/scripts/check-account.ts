// make check-account ID=<account> [NETWORK=testnet]: the check_account MCP
// tool from the command line. Prints the verdict and findings; exit 0
// locked, 1 not locked (key-controlled, unknown code, backdoor risk), 2 error.
import { connect } from "../src/client.js";

const id = process.argv[2];
if (!id || process.argv.length > 3 || /\s/.test(id)) {
  process.stderr.write("usage: check-account <account>\n");
  process.exit(2);
}
const s = await connect({ AGENT_EXIT_NETWORK: process.env.AGENT_EXIT_NETWORK ?? "offline" });
try {
  const r = await s.call("check_account", { account: id });
  const b = r.body as any;
  if (!r.ok) {
    process.stderr.write(`check-account: ${b.error.code}: ${b.error.message}\n`);
    process.exit(2);
  }
  process.stdout.write(`${b.account} (${b.network}): ${b.verdict}\n  ${b.summary}\n`);
  for (const f of b.findings) process.stdout.write(`  [${f.severity}] ${f.code}: ${f.text}\n`);
  for (const k of b.keys) process.stdout.write(`  key ${k.public_key}: ${k.can}\n`);
  process.exitCode = b.verdict === "locked" ? 0 : 1;
} finally {
  await s.close();
}
