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
// The current owner's runner key from the recorded testnet buy (scene 6), so
// check_claims K5 can prove the lockout on testnet (a public key, not a secret).
const scene6 = read("out/scenes/scene-6.json");
const ownerPk = scene6.verify_lockout?.keys?.[0]?.public_key;
if (!/^ed25519:[1-9A-HJ-NP-Za-km-z]{32,44}$/.test(ownerPk ?? "")) throw new Error("out/scenes/scene-6.json has no verify_lockout public key");
const r = deploy.root;
// After testnet scene 9 the born-clean factory and its first agent are named too.
let born = null;
try { born = read("out/scenes/scene-9.json"); } catch {}
if (born && !(born.pass === true && /^[a-z0-9_-]+\.factory\.[a-z0-9._-]+\.testnet$/.test(born.agent ?? "") && born.agent.endsWith(`.${born.factory}`))) born = null;
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
        AGENT_EXIT_AGENTS: [...["translator", "translator3", "cheat"].map((a) => `${a}.${r}`), ...(born ? [born.agent] : [])].join(","),
        AGENT_EXIT_TOKEN: `xusd.${r}`,
        AGENT_EXIT_ADMIN: r,
        AGENT_EXIT_CODE_HASH: deploy.agent_code_hash,
        // FastNEAR: rpc.testnet.near.org rate-limits (HTTP 429) the K5 reads.
        AGENT_EXIT_RPC: "https://rpc.testnet.fastnear.com",
        AGENT_EXIT_RPC_2: "https://test.rpc.fastnear.com",
        AGENT_EXIT_RUNNER_PK: ownerPk,
        ...(born ? { AGENT_EXIT_FACTORY: born.factory } : {}),
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
