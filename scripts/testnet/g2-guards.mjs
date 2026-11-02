// Safety checks for the G2 relay, kept apart from the network code so they
// can be tested offline. Testnet only; secrets are never printed.
import { statSync } from "node:fs";

export const CHAIN_ID = 84532; // Base Sepolia
export const RPC_HOSTS = ["sepolia.base.org", "base-sepolia-rpc.publicnode.com", "base-sepolia.drpc.org"];
// transferWithAuthorization(address,address,uint256,uint256,uint256,bytes32,uint8,bytes32,bytes32)
export const TRANSFER_WITH_AUTH = "0xe3ee160e";
// Test USDC only; a gate never needs more than 100 USDC (6 decimals).
export const MAX_VALUE = 100_000_000n;

/** KEY=VALUE lines of the env file; `export ` and quotes are accepted. */
export function parseEnvFile(text) {
  const out = {};
  for (const line of text.split("\n")) {
    const l = line.trim().replace(/^export /, "");
    if (!l || l.startsWith("#") || !l.includes("=")) continue;
    const i = l.indexOf("=");
    out[l.slice(0, i).trim()] = l.slice(i + 1).trim().replace(/^(["'])(.*)\1$/, "$2");
  }
  return out;
}

/** Throws unless `rpc` is an https Base Sepolia host from the allowlist. The
 * message never carries the URL path or query, which may hold an API key. */
export function checkBaseRpc(rpc) {
  let u;
  try {
    u = new URL(rpc);
  } catch {
    throw new Error("refusing Base RPC: not a URL");
  }
  if (u.protocol !== "https:" || u.port || u.username || u.password || !RPC_HOSTS.includes(u.hostname)) {
    throw new Error(`refusing Base RPC on host ${u.hostname || "?"}: only ${RPC_HOSTS.join(", ")} over https`);
  }
  return rpc;
}

/** Throws unless the value looks like a 32-byte hex key. Never echoes it. */
export function checkRelayerKey(key) {
  if (!key) throw new Error("BASE_RELAYER_KEY missing");
  if (!/^(0x)?[0-9a-fA-F]{64}$/.test(key)) throw new Error("BASE_RELAYER_KEY is not a 32-byte hex key (value not shown)");
  return key.startsWith("0x") ? key : `0x${key}`;
}

/** Only the EIP-3009 transfer, and only for small test amounts, gets relayed.
 * The value, payee, payer and nonce are decoded from the calldata itself and
 * must equal the JSON fields: the JSON is not what the chain executes. */
export function checkAuthorization(auth) {
  if (typeof auth?.calldata !== "string" || !auth.calldata.toLowerCase().startsWith(TRANSFER_WITH_AUTH)) {
    throw new Error(`calldata is not transferWithAuthorization (${TRANSFER_WITH_AUTH}): refusing to relay`);
  }
  let value;
  try {
    value = BigInt(auth.value);
  } catch {
    throw new Error("authorization value is not a number");
  }
  if (value <= 0n || value > MAX_VALUE) throw new Error(`authorization value ${value} outside 1..${MAX_VALUE} base units: refusing to relay`);
  const hex = auth.calldata.slice(2 + 8);
  if (!/^[0-9a-fA-F]*$/.test(hex) || hex.length !== 9 * 64) {
    throw new Error(`calldata length is not transferWithAuthorization's 9 words: refusing to relay`);
  }
  const w = (i) => BigInt("0x" + hex.slice(i * 64, i * 64 + 64));
  const same = (name, got, want) => {
    let ok = false;
    try {
      ok = got === BigInt(want);
    } catch {
      /* not a number */
    }
    if (!ok) throw new Error(`calldata ${name} ${name === "value" ? got : "0x" + got.toString(16)} != auth.${name} ${want}: refusing to relay`);
  };
  same("value", w(2), auth.value);
  if (w(2) <= 0n || w(2) > MAX_VALUE) throw new Error(`calldata value ${w(2)} outside 1..${MAX_VALUE} base units: refusing to relay`);
  for (const [name, i] of [["from", 0], ["to", 1], ["nonce", 5]]) if (auth[name] !== undefined) same(name, w(i), auth[name]);
  return auth;
}

/** AGENT_EXIT_NETWORK must be present and equal to testnet. */
export function requireTestnet(env) {
  if (!env.AGENT_EXIT_NETWORK) throw new Error("AGENT_EXIT_NETWORK is not set: set it to testnet");
  if (env.AGENT_EXIT_NETWORK !== "testnet") throw new Error("AGENT_EXIT_NETWORK is not testnet");
}

/** The chainsig.js derivation must have run and agree with the contract. */
export function checkChainsig(result) {
  if (result.chainsig_js_matches === true) return;
  if (result.chainsig_js_matches === false) throw new Error("the chainsig.js address does not match the contract address: refusing to relay");
  throw new Error("could not derive the address with chainsig.js, so the contract's address is unverified: refusing to relay");
}

/** A file that holds secrets must not be readable by group or others.
 * A missing file is fine (the caller decides if it is needed). */
export function checkSecretFileMode(path) {
  let st;
  try {
    st = statSync(path);
  } catch {
    return;
  }
  if (st.mode & 0o077) {
    throw new Error(`${path} is mode ${(st.mode & 0o777).toString(8)}: run chmod 600 on it (values not shown)`);
  }
}

/** Replace each secret (and the RPC path, which may carry a key) in `text`. */
export function scrub(text, secrets) {
  let t = String(text);
  for (const s of secrets) if (s && String(s).length >= 8) t = t.split(String(s)).join("[redacted]");
  return t;
}

export const DRY_RUN_PLAN = [
  "1. check ~/.agent-exit.env is mode 600, AGENT_EXIT_NETWORK is set to testnet and BASE_SEPOLIA_RPC is an allowlisted https Base Sepolia host",
  "2. read out/gates/g2-sign.json (made by make gate-g2) and check the calldata is transferWithAuthorization for at most 100 test USDC, with value, payer, payee and nonce equal to the JSON fields",
  "3. derive the agent's Base Sepolia address with chainsig.js and compare it with the contract's (a mismatch stops the relay)",
  "4. check chain id 84532, the USDC domain separator, the balance, the nonce and the validity window",
  "5. send the calldata from BASE_RELAYER_KEY (never printed) and write out/gates/g2.json",
];
