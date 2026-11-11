// `make judge`: what a judge's agent sees first. Starts the MCP server from
// the repo's .mcp.json (offline entry), calls check_claims and prints one row
// per README claim. Writes out/check-claims.json. Exit 0 all pass, 1 on any
// failed claim, 3 when some testnet claims could not be checked because no
// RPC answered (nothing read differs; this is not a failed claim).
import { mkdirSync, readFileSync, writeFileSync } from "node:fs";
import { join } from "node:path";
import { connect, REPO } from "../src/client.js";

async function main(): Promise<number> {
  const cfg = JSON.parse(readFileSync(join(REPO, ".mcp.json"), "utf8"));
  const testnet = process.argv.includes("--testnet");
  const entry = testnet ? "agent-exit-testnet" : "agent-exit";
  const s = await connect(cfg.mcpServers[entry].env);
  try {
    const tools = (await s.client.listTools()).tools.map((t) => t.name);
    const prompts = (await s.client.listPrompts()).prompts.map((p) => p.name);
    const r = await s.call("check_claims", {});
    if (!r.ok) {
      process.stderr.write(`judge: check_claims failed: ${JSON.stringify(r.body)}\n`);
      return 1;
    }
    process.stderr.write(`judge: MCP server from .mcp.json: ${tools.length} tools (${tools.join(", ")}), prompt ${prompts.join(", ")}\n`);
    const label = (c: { status?: string; pass: boolean | null }) => (c.status === "unverified_network" ? "NOT CHECKED (no RPC)" : c.pass ? "PASS" : "FAIL");
    for (const c of r.body.claims) process.stderr.write(`${label(c)} ${c.id} ${c.claim}${c.world && c.world !== "offline" ? ` [${c.world}]` : ""}\n     ${c.observed}\n`);
    if (r.body.note) process.stderr.write(`judge: ${r.body.note}\n`);
    const unv = r.body.unverified ? `, ${r.body.unverified} not checked (no RPC)` : "";
    process.stderr.write(`judge: ${r.body.passed}/${r.body.claims.length} claims pass${unv} (${testnet ? "live NEAR testnet, archival transactions + live reads" : "offline sandbox, real contract code"}, xUSD-FAKE)\n`);
    mkdirSync(join(REPO, "out"), { recursive: true });
    writeFileSync(join(REPO, "out", testnet ? "check-claims-testnet.json" : "check-claims.json"), JSON.stringify({ tools, prompts, ...r.body }, null, 2) + "\n");
    return r.body.pass === true ? 0 : r.body.pass === null ? 3 : 1;
  } finally {
    await s.close();
  }
}

main().then((c) => process.exit(c), (e) => {
  process.stderr.write(`${(e as Error).stack}\n`);
  process.exit(1);
});
