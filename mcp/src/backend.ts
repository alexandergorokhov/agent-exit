// Where the tools get their data.
//
// - OfflineBackend (default): reads fixtures/mcp/world.json, which is
//   RECORDED from the real contract code in the mocked runtime
//   (`cargo run -q -p verify --bin gen-mcp-world`). Writes replay recorded
//   contract results in memory; nothing is sent anywhere.
// - TestnetBackend (AGENT_EXIT_NETWORK=testnet): read-only RPC views from two
//   providers. Writes on testnet are not built yet and say so.
import { readFileSync } from "node:fs";
import type { Config, Network } from "./config.js";

export type Json = any; // eslint-disable-line @typescript-eslint/no-explicit-any

export class ToolError extends Error {
  constructor(
    public code: string,
    message: string,
    public details?: Json,
  ) {
    super(message);
  }
}

export interface AgentView {
  status: Json;
  earnings: Json;
  listing: Json | null;
  limits: Json;
  sales: Json[];
  /** Access keys on the account, scan-input form. */
  access_keys: Json[];
  /** Public key -> account that used it, when known. */
  key_owners: Record<string, string>;
}

export interface BuyOutcome {
  sale: Json;
  key_batch: { action: string; key: string }[];
  overpay_refund: string;
  recorded: boolean;
}

/** What the buyer was quoted, from one `get_listing` read (A32). */
export interface ScanExpect {
  listing_version?: number;
  price?: string;
  min_terms?: MinTerms;
}

export interface MinTerms {
  min_earnout_bps: number;
  max_floor_bps: number;
  min_window_secs: number;
}

/** get_listing -> scan-input `listing` (version, price and terms of one read). */
export function listingView(l: Json | null): Json | undefined {
  if (!l) return undefined;
  const t = l.terms;
  return {
    listing_version: l.listing_version,
    price: l.price,
    terms: { earnout_bps: t.earnout_bps, share_bps: t.share_bps, floor_bps: t.floor_bps, window_secs: t.window_secs },
  };
}

function applyExpect(input: Json, e: ScanExpect): Json {
  if (e.listing_version !== undefined) input.expected.listing_version = e.listing_version;
  if (e.price !== undefined) input.expected.price = e.price;
  if (e.min_terms !== undefined) input.expected.min_terms = e.min_terms;
  return input;
}

export interface Backend {
  network: Network;
  listAgents(): Promise<string[]>;
  agent(id: string): Promise<AgentView>;
  scanInput(id: string, expect: ScanExpect): Promise<Json>;
  buy(id: string, buyer: string, pk: string, amount: string, version: number, hash: string): Promise<BuyOutcome>;
  list(id: string, seller: string, price: string): Promise<{ listing: Json; recorded: boolean }>;
  /** Throws ALLOWANCE_LOW unless the function-call key has at least `min` yoctoNEAR of allowance (no-op offline). */
  assertAllowance(account: string, publicKey: string, min: bigint): Promise<void>;
  /** Results of owner-only calls attempted by `account`, when known. */
  probes(id: string, account: string): Promise<Record<string, string> | null>;
}

// ------------------------------------------------------------------ offline

interface RecordedAgent extends AgentView {
  scan_input: Json;
}

export class OfflineBackend implements Backend {
  network: Network = "offline";
  private world: Json;
  private agents: Map<string, RecordedAgent>;
  /** agent -> recorded buy applied (for the lockout probes). */
  private bought = new Map<string, Json>();

  constructor(path: string) {
    this.world = JSON.parse(readFileSync(path, "utf8"));
    if (this.world.schema !== "mcp-world.v1") throw new Error(`${path} is not mcp-world.v1`);
    this.agents = new Map(Object.entries(this.world.agents) as [string, RecordedAgent][]);
  }

  async listAgents(): Promise<string[]> {
    return [...this.agents.keys()].sort();
  }

  private get(id: string): RecordedAgent {
    const a = this.agents.get(id);
    if (!a) throw new ToolError("UNKNOWN_AGENT", `no agent ${id} in the offline world`);
    return a;
  }

  async agent(id: string): Promise<AgentView> {
    return structuredClone(this.get(id));
  }

  async scanInput(id: string, expect: ScanExpect): Promise<Json> {
    return applyExpect(structuredClone(this.get(id).scan_input), expect);
  }

  async buy(id: string, buyer: string, pk: string, amount: string, version: number, hash: string): Promise<BuyOutcome> {
    const rec = (this.world.recorded_buys as Json[]).find(
      (b) =>
        b.agent === id &&
        b.buyer === buyer &&
        b.buyer_runner_pk === pk &&
        b.amount === amount &&
        b.listing_version === version &&
        b.known_keys_hash === hash,
    );
    if (!rec) {
      throw new ToolError(
        "OFFLINE_NO_RECORDING",
        "offline mode replays recorded contract results only; this buy (agent, buyer, key, amount, version, hash) was not recorded",
        { recorded: (this.world.recorded_buys as Json[]).map((b) => ({ agent: b.agent, buyer: b.buyer, buyer_runner_pk: b.buyer_runner_pk })) },
      );
    }
    this.agents.set(id, structuredClone(rec.after));
    this.bought.set(id, rec);
    return {
      sale: rec.sale[0],
      key_batch: rec.key_batch,
      overpay_refund: rec.overpay_refund,
      recorded: true,
    };
  }

  async list(id: string, seller: string, price: string): Promise<{ listing: Json; recorded: boolean }> {
    const rec = (this.world.recorded_listings as Json[]).find(
      (l) => l.agent === id && l.seller === seller && l.price === price,
    );
    if (!rec || this.get(id).status.owner !== seller) {
      throw new ToolError("OFFLINE_NO_RECORDING", "offline mode replays recorded listings only; this one was not recorded", {
        recorded: (this.world.recorded_listings as Json[]).map((l) => ({ agent: l.agent, seller: l.seller, price: l.price })),
      });
    }
    this.agents.set(id, structuredClone(rec.after));
    return { listing: rec.listing, recorded: true };
  }

  async assertAllowance(): Promise<void> {}

  async probes(id: string, account: string): Promise<Record<string, string> | null> {
    const rec = this.bought.get(id);
    if (rec && rec.old_owner_probes.account === account) return rec.old_owner_probes.results;
    return null;
  }
}

// ------------------------------------------------------------------ testnet

const MAX_EXPORTS = 10_000;

/** Function export names of a wasm module (the deployed contract's methods).
 * Every read stays inside the buffer and inside its section; a malformed
 * module throws instead of spinning (L5). */
export function wasmExports(code: Uint8Array): string[] {
  const bad = (why: string): never => {
    throw new ToolError("RPC_ERROR", `malformed wasm: ${why}`);
  };
  if (code.length < 8) bad("too short");
  let i = 8;
  let limit = code.length;
  const byte = (): number => {
    if (i >= limit) bad("read past the end");
    return code[i++];
  };
  const leb = (): number => {
    let r = 0;
    for (let shift = 0; shift < 35; shift += 7) {
      const b = byte();
      r += (b & 0x7f) * 2 ** shift;
      if ((b & 0x80) === 0) {
        if (r > 0xffffffff) bad("integer too large");
        return r;
      }
    }
    return bad("integer too long");
  };
  const names: string[] = [];
  while (i < code.length) {
    limit = code.length;
    const id = byte();
    const size = leb();
    const end = i + size;
    if (end > code.length) bad("section longer than the module");
    if (id === 7) {
      limit = end;
      const n = leb();
      if (n > MAX_EXPORTS) bad("too many exports");
      for (let k = 0; k < n; k++) {
        const len = leb();
        if (i + len > end) bad("name longer than its section");
        const name = new TextDecoder().decode(code.subarray(i, i + len));
        i += len;
        const kind = byte();
        leb();
        if (kind === 0) names.push(name);
      }
    }
    i = end;
  }
  return names;
}

/** The Chain Signatures contract the published agent wasm is built for. */
export const MPC_SIGNER = "v1.signer-prod.testnet";

export class TestnetBackend implements Backend {
  network: Network = "testnet";
  constructor(
    private cfg: Config,
    private fetchImpl: typeof fetch = fetch,
  ) {}

  private async rpc(url: string, params: Json): Promise<Json> {
    let res: Response;
    try {
      res = await this.fetchImpl(url, {
        method: "POST",
        headers: { "content-type": "application/json" },
        body: JSON.stringify({ jsonrpc: "2.0", id: "agent-exit", method: "query", params }),
        // L5: a redirect could send a read to a host that is not allowlisted.
        redirect: "error",
        signal: AbortSignal.timeout(this.cfg.rpcTimeoutMs),
      });
    } catch (e) {
      throw new ToolError("RPC_ERROR", `RPC ${url} unreachable: ${(e as Error).message}`);
    }
    if (!res.ok) throw new ToolError("RPC_ERROR", `RPC ${url}: HTTP ${res.status}`);
    let body: Json;
    try {
      body = await res.json();
    } catch {
      throw new ToolError("RPC_ERROR", `RPC ${url}: reply is not JSON`);
    }
    if (body.error || body.result?.error) {
      throw new ToolError("RPC_ERROR", `RPC ${url}: ${JSON.stringify(body.error ?? body.result.error)}`);
    }
    return body.result;
  }

  private async view(id: string, method: string, args: Json = {}): Promise<Json> {
    const r = await this.rpc(this.cfg.rpc[0], {
      request_type: "call_function",
      finality: "final",
      account_id: id,
      method_name: method,
      args_base64: Buffer.from(JSON.stringify(args)).toString("base64"),
    });
    return JSON.parse(Buffer.from(r.result).toString("utf8"));
  }

  private async keys(url: string, id: string): Promise<Json[]> {
    const r = await this.rpc(url, { request_type: "view_access_key_list", finality: "final", account_id: id });
    return (r.keys as Json[]).map((k) => {
      const p = k.access_key.permission;
      return {
        public_key: k.public_key,
        permission:
          p === "FullAccess"
            ? "FullAccess"
            : { receiver_id: p.FunctionCall.receiver_id, method_names: p.FunctionCall.method_names, allowance: p.FunctionCall.allowance },
      };
    });
  }

  async listAgents(): Promise<string[]> {
    return [...this.cfg.agents].sort();
  }

  private known(id: string): void {
    if (!id.endsWith(".testnet")) throw new ToolError("NETWORK_REFUSED", `${id} is not a .testnet account`);
  }

  async agent(id: string): Promise<AgentView> {
    this.known(id);
    const [status, earnings, listing, limits, sales, keys] = await Promise.all([
      this.view(id, "get_status"),
      this.view(id, "get_earnings"),
      this.view(id, "get_listing"),
      this.view(id, "get_limits"),
      this.view(id, "get_sales"),
      this.keys(this.cfg.rpc[0], id),
    ]);
    return { status, earnings, listing, limits, sales, access_keys: keys, key_owners: {} };
  }

  /** A51: the agent's Base wallet as it reports it, plus the MPC root key read
   * from the published signer contract on both RPC providers. The key is
   * never taken from the agent, and the agent cannot name a different signer.
   * Anything that cannot be read leaves the field out or null, and the scan
   * blocks it as base_wallet_unchecked. */
  private async baseWallet(id: string): Promise<Json | undefined> {
    let w: Json;
    try {
      w = await this.view(id, "get_base_wallet");
    } catch {
      return undefined;
    }
    let mpc: string | null = null;
    if (w.mpc === MPC_SIGNER) {
      const read = async (url: string): Promise<string | null> => {
        try {
          const r = await this.rpc(url, {
            request_type: "call_function",
            finality: "final",
            account_id: MPC_SIGNER,
            method_name: "public_key",
            args_base64: Buffer.from("{}").toString("base64"),
          });
          const pk = JSON.parse(Buffer.from(r.result).toString("utf8"));
          return typeof pk === "string" && /^secp256k1:[1-9A-HJ-NP-Za-km-z]{80,100}$/.test(pk) ? pk : null;
        } catch {
          return null;
        }
      };
      const [a, b] = await Promise.all([read(this.cfg.rpc[0]), read(this.cfg.rpc[1])]);
      mpc = a !== null && a === b ? a : null; // two providers must agree
    }
    return { address: typeof w.address === "string" ? w.address : null, path: String(w.path ?? ""), mpc_public_key: mpc };
  }

  async scanInput(id: string, expect: ScanExpect): Promise<Json> {
    this.known(id);
    const [status, listing, keys1, keys2, account, code, baseWallet] = await Promise.all([
      this.view(id, "get_status"),
      this.view(id, "get_listing"),
      this.keys(this.cfg.rpc[0], id),
      this.keys(this.cfg.rpc[1], id),
      this.rpc(this.cfg.rpc[0], { request_type: "view_account", finality: "final", account_id: id }),
      this.rpc(this.cfg.rpc[0], { request_type: "view_code", finality: "final", account_id: id }),
      this.baseWallet(id),
    ]);
    return applyExpect({
      schema: "scan-input.v1",
      agent: id,
      now_ns: Date.now() * 1_000_000,
      expected: {
        // Unset = the scan reports code_hash_mismatch and the buy is refused.
        code_hash: this.cfg.expectedCodeHash ?? "unset: AGENT_EXIT_CODE_HASH",
        // Unset (a plain scan_agent call) = the current version.
        listing_version: status.listing_version,
        // The published release's registry admin and token (A26); unset =
        // the scanner's published constants.
        ...(this.cfg.expectedAdmin ? { admin: this.cfg.expectedAdmin } : {}),
        ...(this.cfg.expectedToken ? { token: this.cfg.expectedToken } : {}),
      },
      chain: {
        code_hash: account.code_hash,
        methods: wasmExports(Buffer.from(code.code_base64, "base64")),
        keys: keys1,
        keys_second_provider: keys2,
        subaccounts: [],
      },
      contract: {
        status: status.status,
        owner: status.owner,
        listing_version: status.listing_version,
        known_keys: status.known_keys,
        known_keys_hash: status.known_keys_hash,
        open_approvals: status.open_approvals,
        open_auth_until: 0,
        frozen: status.frozen,
        admin: status.admin,
        token: status.token,
        paid_to_total: status.paid_to_total,
        near_storage_solvent: status.near_storage?.solvent,
        ...(baseWallet ? { base_wallet: baseWallet } : {}),
      },
      ...(listing ? { listing: listingView(listing) } : {}),
    }, expect);
  }

  async buy(): Promise<BuyOutcome> {
    throw new ToolError("NOT_BUILT", "not built yet: testnet buy through MCP (planned with the owner's testnet run); nothing was sent");
  }

  async list(): Promise<{ listing: Json; recorded: boolean }> {
    throw new ToolError("NOT_BUILT", "not built yet: testnet list_for_sale through MCP; nothing was sent");
  }

  /** On a real node an allowance failure surfaces only as InvalidTransaction({}),
   * so read the key first and refuse with a reason. */
  async assertAllowance(account: string, publicKey: string, min: bigint): Promise<void> {
    this.known(account);
    let r: Json;
    try {
      r = await this.rpc(this.cfg.rpc[0], { request_type: "view_access_key", finality: "final", account_id: account, public_key: publicKey });
    } catch (e) {
      throw new ToolError("ALLOWANCE_LOW", `cannot read the key ${publicKey} on ${account}: ${(e as Error).message}`);
    }
    const p = r.permission;
    if (p === "FullAccess") return;
    const allowance = p?.FunctionCall?.allowance;
    if (allowance === null) return; // unlimited
    if (typeof allowance !== "string" || !/^[0-9]+$/.test(allowance)) {
      throw new ToolError("ALLOWANCE_LOW", `unreadable allowance on ${publicKey}`);
    }
    if (BigInt(allowance) < min) {
      throw new ToolError("ALLOWANCE_LOW", `key ${publicKey} on ${account} has allowance ${allowance} < ${min} yoctoNEAR: top it up before sending (the node would only say InvalidTransaction)`, {
        allowance,
        needed: min.toString(),
      });
    }
  }

  async probes(): Promise<Record<string, string> | null> {
    return null;
  }
}

export function makeBackend(cfg: Config): Backend {
  return cfg.network === "testnet" ? new TestnetBackend(cfg) : new OfflineBackend(cfg.worldPath);
}
