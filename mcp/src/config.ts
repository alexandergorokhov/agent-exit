// Server configuration, read once from the environment. No keys are ever
// read from files and nothing here can select mainnet.
import { existsSync } from "node:fs";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";

export type Network = "offline" | "testnet";

export interface Config {
  network: Network;
  repo: string;
  worldPath: string;
  allowWrite: boolean;
  /** The account this server acts as (buyer or seller). Writes need it. */
  accountId: string | null;
  /** Testnet only: signing key (never logged, never written). */
  hasPrivateKey: boolean;
  /** Most this server may spend in one session, in xUSD-FAKE dollars. */
  sessionCap: number;
  rpc: [string, string];
  agents: string[];
  expectedCodeHash: string | null;
  expectedAdmin: string | null;
  expectedToken: string | null;
  scanBin: string | null;
}

export class ConfigError extends Error {}

/** Repo root: AGENT_EXIT_REPO, else three levels up from dist/src/config.js. */
function repoRoot(): string {
  if (process.env.AGENT_EXIT_REPO) return resolve(process.env.AGENT_EXIT_REPO);
  const here = dirname(fileURLToPath(import.meta.url));
  return resolve(here, "..", "..", "..");
}

function findScan(repo: string): string | null {
  const env = process.env.AGENT_EXIT_SCAN_BIN;
  if (env) return existsSync(env) ? env : null;
  for (const p of ["target/debug/scan", "target/release/scan"]) {
    const full = join(repo, p);
    if (existsSync(full)) return full;
  }
  return null;
}

/** Refuse anything that is not testnet. */
export function assertNotMainnet(url: string): void {
  const u = url.toLowerCase();
  if (u.includes("mainnet") || !u.startsWith("https://")) {
    throw new ConfigError(`refusing RPC ${url}: testnet over https only, never mainnet`);
  }
}

export function loadConfig(env: NodeJS.ProcessEnv = process.env): Config {
  const network = (env.AGENT_EXIT_NETWORK ?? "offline").toLowerCase();
  if (network !== "offline" && network !== "testnet") {
    throw new ConfigError(
      `AGENT_EXIT_NETWORK=${network} refused: only "offline" (default) or "testnet"; mainnet is never supported`,
    );
  }
  const repo = repoRoot();
  const rpc: [string, string] = [
    env.AGENT_EXIT_RPC ?? "https://rpc.testnet.near.org",
    env.AGENT_EXIT_RPC_2 ?? "https://test.rpc.fastnear.com",
  ];
  if (network === "testnet") rpc.forEach(assertNotMainnet);
  const agents = (env.AGENT_EXIT_AGENTS ?? "").split(",").map((s) => s.trim()).filter(Boolean);
  for (const a of agents) {
    if (!a.endsWith(".testnet")) throw new ConfigError(`agent ${a} is not a .testnet account`);
  }
  const cap = Number(env.AGENT_EXIT_SESSION_CAP ?? "1500");
  if (!Number.isFinite(cap) || cap < 0) throw new ConfigError("AGENT_EXIT_SESSION_CAP must be a number >= 0");
  return {
    network,
    repo,
    worldPath: env.AGENT_EXIT_WORLD ?? join(repo, "fixtures", "mcp", "world.json"),
    allowWrite: env.AGENT_EXIT_ALLOW_WRITE === "1",
    accountId: env.AGENT_EXIT_ACCOUNT_ID?.trim() || null,
    hasPrivateKey: Boolean(env.AGENT_EXIT_PRIVATE_KEY),
    sessionCap: cap,
    rpc,
    agents,
    expectedCodeHash: env.AGENT_EXIT_CODE_HASH ?? null,
    expectedAdmin: env.AGENT_EXIT_ADMIN ?? null,
    expectedToken: env.AGENT_EXIT_TOKEN ?? null,
    scanBin: findScan(repo),
  };
}
