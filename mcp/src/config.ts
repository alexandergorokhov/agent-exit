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
  /** Most of an agent's earnings that may already be pledged to investors
   * (open share slices, basis points) for buy_agent to buy it (review H2). */
  maxSharesBps: number;
  /** Where the spend per account and UTC day is kept across restarts (null: memory only).
   * Testnet defaults to out/mcp-spend.json; AGENT_EXIT_SPEND_FILE overrides. */
  spendFile: string | null;
  rpc: [string, string];
  agents: string[];
  expectedCodeHash: string | null;
  expectedAdmin: string | null;
  expectedToken: string | null;
  scanBin: string | null;
  /** Per-request RPC timeout in ms (L5). */
  rpcTimeoutMs: number;
  /** Public key of the key that signs the server's writes; when set, its allowance is checked before a send. */
  signerPk: string | null;
  /** Least function-call allowance (yoctoNEAR) the signer key needs before a send. */
  minAllowance: bigint;
  /** Least earn-out terms buy_agent accepts (A32); a buy_agent call may only tighten them. */
  minTerms: { min_earnout_bps: number; max_floor_bps: number; min_window_secs: number };
  /** Least price list_for_sale accepts, in xUSD-FAKE dollars (AGENT_EXIT_MIN_LIST_PRICE; review 5, M4). */
  minListPrice: number | null;
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

function findScan(repo: string, e: NodeJS.ProcessEnv): string | null {
  const env = e.AGENT_EXIT_SCAN_BIN;
  if (env) return existsSync(env) ? env : null;
  for (const p of ["target/debug/scan", "target/release/scan"]) {
    const full = join(repo, p);
    if (existsSync(full)) return full;
  }
  // A shared cargo target dir (CARGO_TARGET_DIR) keeps the binary elsewhere.
  if (e.CARGO_TARGET_DIR) {
    for (const p of ["debug/scan", "release/scan"]) {
      const full = join(e.CARGO_TARGET_DIR, p);
      if (existsSync(full)) return full;
    }
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
  const accountId = env.AGENT_EXIT_ACCOUNT_ID?.trim() || null;
  if (accountId && !accountId.endsWith(".testnet")) throw new ConfigError(`AGENT_EXIT_ACCOUNT_ID ${accountId} is not a .testnet account`);
  const pkRe = /^ed25519:[1-9A-HJ-NP-Za-km-z]{32,44}$/;
  const signerPk = env.AGENT_EXIT_SIGNER_PK?.trim() || null;
  if (signerPk && !pkRe.test(signerPk)) throw new ConfigError("AGENT_EXIT_SIGNER_PK must be an ed25519 public key (ed25519:<base58>)");
  const rpcTimeoutMs = Number(env.AGENT_EXIT_RPC_TIMEOUT_MS ?? "10000");
  if (!Number.isInteger(rpcTimeoutMs) || rpcTimeoutMs < 1) throw new ConfigError("AGENT_EXIT_RPC_TIMEOUT_MS must be a whole number >= 1");
  const minAllowanceRaw = env.AGENT_EXIT_MIN_ALLOWANCE ?? "10000000000000000000000"; // 0.01 NEAR
  if (!/^[0-9]{1,30}$/.test(minAllowanceRaw)) throw new ConfigError("AGENT_EXIT_MIN_ALLOWANCE must be yoctoNEAR digits");
  const runnerPk = env.AGENT_EXIT_RUNNER_PK?.trim() || null;
  if (runnerPk && !/^ed25519:[1-9A-HJ-NP-Za-km-z]{32,44}$/.test(runnerPk)) {
    throw new ConfigError("AGENT_EXIT_RUNNER_PK must be an ed25519 public key (ed25519:<base58>)");
  }
  const minWindow = Number(env.AGENT_EXIT_MIN_WINDOW_SECS ?? PUBLISHED_MIN_TERMS.min_window_secs);
  if (!Number.isInteger(minWindow) || minWindow < 60) throw new ConfigError("AGENT_EXIT_MIN_WINDOW_SECS must be a whole number >= 60");
  const cap = Number(env.AGENT_EXIT_SESSION_CAP ?? "1500");
  if (!Number.isFinite(cap) || cap < 0) throw new ConfigError("AGENT_EXIT_SESSION_CAP must be a number >= 0");
  const maxSharesBps = Number(env.AGENT_EXIT_MAX_SHARES_BPS ?? "2500");
  if (!Number.isInteger(maxSharesBps) || maxSharesBps < 0 || maxSharesBps > 10_000) {
    throw new ConfigError("AGENT_EXIT_MAX_SHARES_BPS must be a whole number from 0 to 10000");
  }
  const minListRaw = env.AGENT_EXIT_MIN_LIST_PRICE?.trim();
  const minListPrice = minListRaw ? Number(minListRaw) : null;
  if (minListPrice !== null && (!Number.isFinite(minListPrice) || minListPrice < 0)) throw new ConfigError("AGENT_EXIT_MIN_LIST_PRICE must be a number >= 0");
  return {
    network,
    repo,
    worldPath: env.AGENT_EXIT_WORLD ?? join(repo, "fixtures", "mcp", "world.json"),
    allowWrite: env.AGENT_EXIT_ALLOW_WRITE === "1",
    accountId,
    hasPrivateKey: Boolean(env.AGENT_EXIT_PRIVATE_KEY),
    runnerPk,
    sessionCap: cap,
    maxSharesBps,
    spendFile: env.AGENT_EXIT_SPEND_FILE || (network === "testnet" ? join(repo, "out", "mcp-spend.json") : null),
    rpc,
    agents,
    expectedCodeHash: env.AGENT_EXIT_CODE_HASH ?? null,
    expectedAdmin: env.AGENT_EXIT_ADMIN ?? null,
    expectedToken: env.AGENT_EXIT_TOKEN ?? null,
    scanBin: findScan(repo, env),
    rpcTimeoutMs,
    signerPk,
    minAllowance: BigInt(minAllowanceRaw),
    minTerms: { ...PUBLISHED_MIN_TERMS, min_window_secs: minWindow },
    minListPrice,
    termsConfig: minWindow >= PUBLISHED_MIN_TERMS.min_window_secs ? "prod" : "demo",
  };
}
