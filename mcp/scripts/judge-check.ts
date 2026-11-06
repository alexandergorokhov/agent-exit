// `make judge`: what a judge's agent sees first. Starts the MCP server from
// the repo's .mcp.json (offline entry), calls check_claims and prints one row
// per README claim. Writes out/check-claims.json. Exit 1 on any failed claim.
import { mkdirSync, readFileSync, writeFileSync } from "node:fs";
import { join } from "node:path";
import { connect, REPO } from "../src/client.js";

async function main(): Promise<number> {
  const cfg = JSON.parse(readFileSync(join(REPO, ".mcp.json"), "utf8"));
  const s = await connect(cfg.mcpServers["agent-exit"].env);
  try {
    const tools = (await s.client.listTools()).tools.map((t) => t.name);
    const prompts = (await s.client.listPrompts()).prompts.map((p) => p.name);
    const r = await s.call("check_claims", {});
    if (!r.ok) {
      process.stderr.write(`judge: check_claims failed: ${JSON.stringify(r.body)}\n`);
      return 1;
    }
    process.stderr.write(`judge: MCP server from .mcp.json: ${tools.length} tools (${tools.join(", ")}), prompt ${prompts.join(", ")}\n`);
    for (const c of r.body.claims) process.stderr.write(`${c.pass ? "PASS" : "FAIL"} ${c.id} ${c.claim}\n     ${c.observed}\n`);
    process.stderr.write(`judge: ${r.body.passed}/${r.body.claims.length} claims pass (offline sandbox, real contract code, xUSD-FAKE)\n`);
    mkdirSync(join(REPO, "out"), { recursive: true });
    writeFileSync(join(REPO, "out", "check-claims.json"), JSON.stringify({ tools, prompts, ...r.body }, null, 2) + "\n");
    return r.body.pass ? 0 : 1;
  } finally {
    await s.close();
  }
}

main().then((c) => process.exit(c), (e) => {
  process.stderr.write(`${(e as Error).stack}\n`);
  process.exit(1);
});
