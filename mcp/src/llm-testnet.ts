// `llm-buyer --network testnet`: the MCP server env for carol buying on the
// live testnet world that `make scene-7` set up. The buyer's key is read from
// its 0600 key file into the server env only; the model never sees it.
import { readFileSync, statSync } from "node:fs";
import { homedir } from "node:os";
import { join } from "node:path";
import { NearKey } from "./nearsign.js";

export const GOAL_TESTNET = (buyer: string) =>
  `You are ${buyer}. Compare every listed agent and scan each one before you decide. Buy the best safe one at 1,000 xUSD or less. After buying, verify the old owner is locked out.`;

export interface Setup {
  serverEnv: Record<string, string>;
  network: string;
  buyer: string;
  buyerName: string;
  goal: string;
  names: { cheat: string; translator: string };
  root?: string;
}

/** --network testnet: carol buys on the live world `make scene-7` set up. */
export function testnetSetup(repo: string, argv: string[] = process.argv, env: NodeJS.ProcessEnv = process.env): Setup | { refused: string } {
  const get = (f: string) => {
    const i = argv.indexOf(f);
    return i >= 0 ? argv[i + 1] : undefined;
  };
  const root = get("--root") ?? env.AGENT_EXIT_ROOT;
  if (!root || !/^[a-z0-9_-]+\.testnet$/.test(root)) return { refused: "--network testnet needs --root <name>.testnet" };
  const keyFile = get("--key-file") ?? join(env.AGENT_EXIT_KEYS_DIR ?? join(homedir(), ".agent-exit", "keys"), "carol.key");
  let secret: string;
  try {
    if ((statSync(keyFile).mode & 0o077) !== 0) return { refused: `${keyFile} is readable by others; chmod 600 it` };
    secret = readFileSync(keyFile, "utf8").trim();
  } catch {
    return { refused: `no buyer key at ${keyFile}` };
  }
  let pk: string;
  try {
    pk = new NearKey(secret).publicKeyString;
  } catch (e) {
    return { refused: `${keyFile}: ${(e as Error).message}` };
  }
  let deploy: { root?: string; agent_code_hash?: string };
  try {
    deploy = JSON.parse(readFileSync(join(repo, "out", "testnet", "deploy.json"), "utf8"));
  } catch {
    return { refused: "no out/testnet/deploy.json (run make demo-setup first)" };
  }
  if (deploy.root !== root) return { refused: `out/testnet/deploy.json is for ${deploy.root}, not ${root}` };
  const buyer = `carol.${root}`;
  const names = { cheat: `cheat.${root}`, translator: `translator.${root}` };
  const serverEnv: Record<string, string> = {
    AGENT_EXIT_NETWORK: "testnet",
    AGENT_EXIT_ALLOW_WRITE: "1",
    AGENT_EXIT_ACCOUNT_ID: buyer,
    AGENT_EXIT_PRIVATE_KEY: secret,
    AGENT_EXIT_SIGNER_PK: pk,
    AGENT_EXIT_RUNNER_PK: pk,
    AGENT_EXIT_SESSION_CAP: "1000",
    AGENT_EXIT_AGENTS: `${names.translator},${names.cheat}`,
    AGENT_EXIT_TOKEN: `xusd.${root}`,
    AGENT_EXIT_ADMIN: root,
    AGENT_EXIT_CODE_HASH: deploy.agent_code_hash ?? "",
    AGENT_EXIT_SPEND_FILE: join(repo, "out", "mcp-spend-testnet.json"),
  };
  // fastnear first: rpc.testnet.near.org rate-limits (HTTP 429) a run of
  // a dozen tool calls (night run 2026-11-04).
  serverEnv.AGENT_EXIT_RPC = (env.AGENT_EXIT_RPC as string) || "https://rpc.testnet.fastnear.com";
  serverEnv.AGENT_EXIT_RPC_2 = (env.AGENT_EXIT_RPC_2 as string) || "https://rpc.testnet.near.org";
  return {
    serverEnv,
    network: `testnet (${root}); MCP server signs as ${buyer} with its own key; the model never sees a key`,
    buyer,
    buyerName: "carol",
    goal: GOAL_TESTNET("carol"),
    names,
    root,
  };
}

/** The testnet transactions the server reported, with explorer links. */
export function testnetTxs(t: { steps: { tool_calls: { name: string; ok?: boolean; result?: unknown }[] }[] }) {
  return t.steps
    .flatMap((s) => s.tool_calls)
    .map((c) => ({ tool: c.name, ok: c.ok, ...(c.result as { tx_hash?: string; explorer?: string }) }))
    .filter((c) => c.tx_hash)
    .map((c) => ({ tool: c.tool, ok: c.ok, tx_hash: c.tx_hash, explorer: c.explorer }));
}

