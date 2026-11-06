#!/usr/bin/env node
// Writes .mcp.json at the repo root: the MCP servers a judge's or reviewer's
// agent (Claude Code reads this file on its own) gets when it opens this repo.
//   agent-exit          offline: replays results recorded from the real contract
//                       code; writes on as the recorded buyer (nothing is sent).
//   agent-exit-testnet  read-only views of the live testnet agents named in
//                       out/testnet/deploy.json (no key, never mainnet).
// `make web-testnet-data` runs it after a deploy; `--check` fails if stale.
import { readFileSync, writeFileSync } from "node:fs";

const root = new URL("..", import.meta.url);
const read = (p) => JSON.parse(readFileSync(new URL(p, root), "utf8"));
const deploy = read("out/testnet/deploy.json");
const world = read("fixtures/mcp/world.json");
const buy = world.recorded_buys[0];
const r = deploy.root;
if (!/^[a-z0-9._-]+\.testnet$/.test(r)) throw new Error(`deploy root ${r} is not a .testnet account`);

const cfg = {
  mcpServers: {
    "agent-exit": {
      command: "node",
      args: ["mcp/dist/src/server.js"],
      env: {
        AGENT_EXIT_NETWORK: "offline",
        AGENT_EXIT_ALLOW_WRITE: "1",
        AGENT_EXIT_ACCOUNT_ID: buy.buyer,
        AGENT_EXIT_RUNNER_PK: buy.buyer_runner_pk,
      },
    },
    "agent-exit-testnet": {
      command: "node",
      args: ["mcp/dist/src/server.js"],
      env: {
        AGENT_EXIT_NETWORK: "testnet",
        AGENT_EXIT_AGENTS: ["translator", "translator3", "cheat"].map((a) => `${a}.${r}`).join(","),
        AGENT_EXIT_TOKEN: `xusd.${r}`,
        AGENT_EXIT_ADMIN: r,
        AGENT_EXIT_CODE_HASH: deploy.agent_code_hash,
      },
    },
  },
};
const text = JSON.stringify(cfg, null, 2) + "\n";
const out = new URL(".mcp.json", root);
if (process.argv.includes("--check")) {
  let have = "";
  try { have = readFileSync(out, "utf8"); } catch {}
  if (have !== text) { console.error("judge-mcp-config: .mcp.json is stale; run node scripts/judge-mcp-config.mjs"); process.exit(1); }
  console.error("judge-mcp-config: .mcp.json matches out/testnet/deploy.json");
} else {
  writeFileSync(out, text);
  console.error(`judge-mcp-config: wrote .mcp.json (offline + read-only testnet ${r})`);
}
