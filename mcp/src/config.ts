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
  /** The runner key buy_agent installs on a bought agent (A37). Only from
   * the server env (AGENT_EXIT_RUNNER_PK), never from tool input. */
  runnerPk: string | null;
  /** Most this server may spend in one session, in xUSD-FAKE dollars. */
  sessionCap: number;
  rpc: [string, string];
  agents: string[];
  expectedCodeHash: string | null;
  expectedAdmin: string | null;
  expectedToken: string | null;
  scanBin: string | null;
  /** Least earn-out terms buy_agent accepts (A32); a buy_agent call may only tighten them. */
  minTerms: { min_earnout_bps: number; max_floor_bps: number; min_window_secs: number };
  /** "prod" (published 30-day window) or "demo" (AGENT_EXIT_MIN_WINDOW_SECS shortened it). */
  termsConfig: "prod" | "demo";
}

/** The published minimum (same as tools/scan PUBLISHED_MIN_TERMS and the contract). */
export const PUBLISHED_MIN_TERMS = { min_earnout_bps: 3000, max_floor_bps: 5000, min_window_secs: 30 * 86_400 };

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

/** Known NEAR testnet RPC hosts (A41). A mainnet endpoint need not say
 * "mainnet" in its URL, so anything not on this list is refused. */
export const TESTNET_RPC_HOSTS = [
  "rpc.testnet.near.org",
  "archival-rpc.testnet.near.org",
  "test.rpc.fastnear.com",
  "rpc.testnet.fastnear.com",
  "archival-rpc.testnet.fastnear.com",
];

/** Refuse anything that is not a known testnet RPC over https. */
export function assertNotMainnet(url: string): void {
  let host = "";
  let ok = false;
  try {
    const u = new URL(url);
    host = u.hostname.toLowerCase();
    ok = u.protocol === "https:" && !u.username && !u.password && !u.port && TESTNET_RPC_HOSTS.includes(host) && !url.toLowerCase().includes("mainnet");
  } catch {
    ok = false;
  }
  if (!ok) {
    throw new ConfigError(`refusing RPC ${url}: only known testnet RPC hosts over https (${TESTNET_RPC_HOSTS.join(", ")}), never mainnet`);
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
  const runnerPk = env.AGENT_EXIT_RUNNER_PK?.trim() || null;
  if (runnerPk && !/^ed25519:[1-9A-HJ-NP-Za-km-z]{32,44}$/.test(runnerPk)) {
    throw new ConfigError("AGENT_EXIT_RUNNER_PK must be an ed25519 public key (ed25519:<base58>)");
  }
  const minWindow = Number(env.AGENT_EXIT_MIN_WINDOW_SECS ?? PUBLISHED_MIN_TERMS.min_window_secs);
  if (!Number.isInteger(minWindow) || minWindow < 60) throw new ConfigError("AGENT_EXIT_MIN_WINDOW_SECS must be a whole number >= 60");
  const cap = Number(env.AGENT_EXIT_SESSION_CAP ?? "1500");
  if (!Number.isFinite(cap) || cap < 0) throw new ConfigError("AGENT_EXIT_SESSION_CAP must be a number >= 0");
  return {
    network,
    repo,
    worldPath: env.AGENT_EXIT_WORLD ?? join(repo, "fixtures", "mcp", "world.json"),
    allowWrite: env.AGENT_EXIT_ALLOW_WRITE === "1",
    accountId: env.AGENT_EXIT_ACCOUNT_ID?.trim() || null,
    hasPrivateKey: Boolean(env.AGENT_EXIT_PRIVATE_KEY),
    runnerPk,
    sessionCap: cap,
    rpc,
    agents,
    expectedCodeHash: env.AGENT_EXIT_CODE_HASH ?? null,
    expectedAdmin: env.AGENT_EXIT_ADMIN ?? null,
    expectedToken: env.AGENT_EXIT_TOKEN ?? null,
    scanBin: findScan(repo),
    minTerms: { ...PUBLISHED_MIN_TERMS, min_window_secs: minWindow },
    termsConfig: minWindow >= PUBLISHED_MIN_TERMS.min_window_secs ? "prod" : "demo",
  };
}
