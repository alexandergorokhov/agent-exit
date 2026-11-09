// Where the tools get their data.
//
// - OfflineBackend (default): reads fixtures/mcp/world.json, which is
//   RECORDED from the real contract code in the mocked runtime
//   (`cargo run -q -p verify --bin gen-mcp-world`). Writes replay recorded
//   contract results in memory; nothing is sent anywhere.
// - TestnetBackend (AGENT_EXIT_NETWORK=testnet): read-only RPC views from two
//   providers. buy_agent signs and sends one ft_transfer_call with the
//   server's own key (AGENT_EXIT_PRIVATE_KEY, never the model's input);
//   list_for_sale on testnet is not built yet and says so.
import { createHash } from "node:crypto";
import { readFileSync } from "node:fs";
import { dirname, join } from "node:path";
import type { Config, Network } from "./config.js";
import { b58encode, NearKey, signTx } from "./nearsign.js";

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
  /** Open share offers (get_shares). */
  shares: Json[];
}

/** One share buy as the contract settled it. */
export interface ShareOutcome {
  cost: string;
  /** The 1% protocol fee the contract took from the owner's upfront half (share_bought event; "0" on a contract without a fee). */
  protocol_fee: string;
  refund: string;
  recorded: boolean;
  tx_hash?: string;
  explorer?: string;
}

/** The offer terms the investor read (the contract refuses others, T6). */
export interface ShareTerms {
  offer_id: number;
  bps: number;
  price: string;
  end: string;
}

export interface BuyOutcome {
  sale: Json;
  key_batch: { action: string; key: string }[];
  overpay_refund: string;
  recorded: boolean;
  /** Testnet: the transaction hash and its NearBlocks link. */
  tx_hash?: string;
  explorer?: string;
}

/** EVENT_JSON events named `name` in a final transaction outcome. */
export function txEvents(result: Json, name: string): Json[] {
  const out: Json[] = [];
  for (const r of (result?.receipts_outcome as Json[]) ?? []) {
    for (const l of (r?.outcome?.logs as string[]) ?? []) {
      if (!l.startsWith("EVENT_JSON:")) continue;
      try {
        const e = JSON.parse(l.slice("EVENT_JSON:".length));
        if (e.event === name) out.push(...((e.data as Json[]) ?? []));
      } catch {
        /* not ours */
      }
    }
  }
  return out;
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
    // SH7 (review H2): open earnings slices reach the scan, so it can disclose them.
    shares_bps: Number(l.shares_bps ?? 0),
    shares: Array.isArray(l.shares) ? l.shares : [],
  };
}

function applyExpect(input: Json, e: ScanExpect): Json {
  if (e.listing_version !== undefined) input.expected.listing_version = e.listing_version;
  if (e.price !== undefined) input.expected.price = e.price;
  if (e.min_terms !== undefined) input.expected.min_terms = e.min_terms;
  return input;
}

export interface AccountFacts {
  code_hash: string;
  methods: string[];
  keys: Json[];
  /** The second provider's key list (testnet), or null offline. */
  keys_second: Json[] | null;
  /** The second provider's code hash (testnet), or null when unread. */
  code_hash_second?: string | null;
  /** Where the facts come from. */
  source: string;
  /** NEAR global contract the account runs, by owner account id (its owner can swap the code) or by hash. */
  global_contract_account_id?: string | null;
  global_contract_hash?: string | null;
}

export interface Backend {
  network: Network;
  listAgents(): Promise<string[]>;
  agent(id: string): Promise<AgentView>;
  scanInput(id: string, expect: ScanExpect): Promise<Json>;
  /** `deliveryPk`: the buyer's one-time X25519 key (hex) for a listing that sells a sealed config. */
  buy(id: string, buyer: string, pk: string, amount: string, version: number, hash: string, deliveryPk?: string): Promise<BuyOutcome>;
  /** What `check_account` reads about any account: code hash, exported
   * methods, access keys (from two providers on testnet). Null = no account. */
  accountFacts(id: string): Promise<AccountFacts | null>;
  list(id: string, seller: string, price: string): Promise<{ listing: Json; recorded: boolean }>;
  /** Throws ALLOWANCE_LOW unless the function-call key has at least `min` yoctoNEAR of allowance (no-op offline). */
  assertAllowance(account: string, publicKey: string, min: bigint): Promise<void>;
  /** Results of owner-only calls attempted by `account`, when known. */
  probes(id: string, account: string): Promise<Record<string, string> | null>;
  /** The chain's clock in ns (offline: the recorded world's clock). */
  nowNs(): bigint;
  /** get_holdings of one investor on one agent. */
  holdings(id: string, account: string): Promise<Json>;
  /** One ft_transfer_call with a buy_share message. */
  buyShare(id: string, investor: string, seen: ShareTerms, part: number, amount: string): Promise<ShareOutcome>;
  /** Testnet only: one recorded transaction's outcome as an archival node reports it. */
  txOutcome?(hash: string, sender: string): Promise<TxOutcome>;
  /** get_sealed_config of one sale (null: the sale sold no sealed config). */
  sealedConfig(id: string, saleId: number): Promise<Json | null>;
  /** release_upfront or dispute_config, signed by `signer`. */
  sealedCall(id: string, signer: string, method: "release_upfront" | "dispute_config" | "request_preview", args: Json): Promise<SealedCallOutcome>;
  /** SC21: the current listing's preview (get_preview), or null. */
  preview(id: string): Promise<Json | null>;
  /** Offline only: the fixed demo secret of the sealed fixtures. */
  demoDeliverySecret?(id: string): string | null;
}

export interface SealedCallOutcome {
  /** The contract's return value: to_seller (release_upfront) or refund_to_buyer (dispute_config). */
  value: Json;
  recorded: boolean;
  tx_hash?: string;
  explorer?: string;
}

/** What the chain says about one transaction (check_claims on testnet). */
export interface TxOutcome {
  signer: string;
  receiver: string;
  method: string;
  ok: boolean;
  refused_inside: boolean;
  events: string[];
}

/** Archival testnet RPCs: the README's transactions are older than a
 * regular node keeps. Both are on the testnet allowlist (A41). */
export const ARCHIVAL_RPC = ["https://archival-rpc.testnet.fastnear.com", "https://archival-rpc.testnet.near.org"];

/** Parse an RPC `tx` result into a TxOutcome (same reading as the page's Run and verify). */
export function txOutcomeOf(res: Json): TxOutcome {
  const tx = res.transaction ?? {};
  const method = ((tx.actions ?? []) as Json[]).map((a) => a?.FunctionCall?.method_name).find(Boolean) ?? "(no call)";
  const events: string[] = [];
  for (const o of (res.receipts_outcome ?? []) as Json[]) {
    for (const l of (o.outcome?.logs ?? []) as string[]) {
      if (!l.startsWith("EVENT_JSON:")) continue;
      try {
        events.push(String(JSON.parse(l.slice(11)).event));
      } catch {
        /* not ours */
      }
    }
  }
  return {
    signer: String(tx.signer_id ?? ""),
    receiver: String(tx.receiver_id ?? ""),
    method: String(method),
    ok: Boolean(res.status && "SuccessValue" in res.status),
    refused_inside: ((res.receipts_outcome ?? []) as Json[]).some((o) => o.outcome?.status?.Failure),
    events,
  };
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

  constructor(private path: string) {
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

  private sealedFx: Json | null = null;

  private sealedFixtures(): Json {
    if (!this.sealedFx) {
      try {
        this.sealedFx = JSON.parse(readFileSync(join(dirname(this.path), "sealed.json"), "utf8"));
      } catch {
        this.sealedFx = { agents: {} };
      }
    }
    return this.sealedFx as Json;
  }

  demoDeliverySecret(id: string): string | null {
    return this.sealedFixtures().agents?.[id]?.demo_buyer_sk ?? null;
  }

  async sealedConfig(id: string, saleId: number): Promise<Json | null> {
    const e = this.sealedFixtures().agents?.[id];
    if (!e) throw new ToolError("UNKNOWN_AGENT", `${id} has no sealed sale in the offline fixtures (fixtures/mcp/sealed.json: brainy.testnet, swapped.testnet)`);
    if (e.sale_id !== saleId) throw new ToolError("UNKNOWN_SALE", `sale ${saleId} not found on ${id}`);
    const { demo_buyer_sk: _secret, ...view } = e;
    // The fixture's own clock (its deadlines are relative to it).
    return structuredClone({ ...view, now_ns: String(this.sealedFixtures().now) });
  }

  /** The same rules as agent-core release_upfront / dispute_config
   * (SC8-SC14), on the illustrative fixture; the contract code itself runs
   * in the sandbox tool (accept_config, dispute_config). */
  async preview(id: string): Promise<Json | null> {
    return structuredClone((this.get(id) as Json).preview ?? null);
  }

  async sealedCall(id: string, signer: string, method: "release_upfront" | "dispute_config" | "request_preview", args: Json): Promise<SealedCallOutcome> {
    if (method === "request_preview") {
      // Offline: the recorded preview stands in for the block's random seed.
      const a = this.get(id) as Json;
      if (!a.listing?.config?.root) throw new ToolError("CONTRACT_REFUSED", "the contract refused request_preview: NoPreview; nothing changed", { contract_error: "NoPreview" });
      if (a.preview) throw new ToolError("CONTRACT_REFUSED", "the contract refused request_preview: PreviewTaken; nothing changed", { contract_error: "PreviewTaken" });
      a.preview = { schema: "sealed-preview.v1", listing_version: String(a.listing.version), requested_by: signer, chunks: [0], proven: [], complete: false };
      return { value: { listing_version: a.listing.version, chunks: [0] }, recorded: true };
    }
    const e = this.sealedFixtures().agents?.[id];
    if (!e || e.sale_id !== args.sale_id) throw new ToolError("UNKNOWN_SALE", `sale ${args.sale_id} not found on ${id}`);
    const refused = (code: string) => new ToolError("CONTRACT_REFUSED", `the contract refused ${method}: ${code}; nothing changed`, { contract_error: code });
    if (!e.state?.Delivered) throw refused("DisputeClosed");
    const now = BigInt(this.sealedFixtures().now);
    if (method === "release_upfront") {
      if (e.cleared) return { value: { sale_id: e.sale_id, to_seller: "0" }, recorded: true };
      if (signer !== e.buyer && now <= BigInt(e.dispute_until)) throw refused("DisputeOpen");
      e.cleared = true;
      const fee = BigInt(e.price) / 100n;
      return { value: { sale_id: e.sale_id, to_seller: String(BigInt(e.upfront) - fee) }, recorded: true };
    }
    if (signer !== e.buyer) throw refused("NotBuyer");
    if (e.cleared || now > BigInt(e.dispute_until)) throw refused("DisputeClosed");
    const { open, publicKey, commitOf } = await import("./sealed.js");
    const sk = Buffer.from(String(args.delivery_sk), "hex");
    if (sk.length !== 32 || publicKey(sk).toString("hex") !== e.delivery_pk) throw refused("BadDeliveryKey");
    // As the contract: a delivery that cannot be opened is a mismatch.
    let plain: Buffer | null = null;
    try {
      plain = open(sk, Buffer.from(e.state.Delivered.ephemeral_pk, "hex"), Buffer.from(e.ciphertext, "base64"));
    } catch {
      plain = null;
    }
    if (plain && commitOf(plain).hash === e.commit.hash) throw refused("ConfigMatches");
    const refund = BigInt(e.upfront) + BigInt(e.escrow);
    e.state = { Mismatch: { refunded: String(refund) } };
    return { value: { sale_id: e.sale_id, refund_to_buyer: String(refund) }, recorded: true };
  }

  async accountFacts(id: string): Promise<AccountFacts | null> {
    const a = this.agents.get(id) as Json | undefined;
    if (a) {
      const c = a.scan_input.chain;
      return { code_hash: c.code_hash, methods: c.methods, keys: c.keys, keys_second: null, source: "offline world (recorded from the contract code)" };
    }
    if (!this.extra) {
      try {
        this.extra = JSON.parse(readFileSync(join(dirname(this.path), "accounts.json"), "utf8")).accounts;
      } catch {
        this.extra = {};
      }
    }
    const x = (this.extra as Json)[id];
    return x ? { code_hash: x.code_hash, methods: x.methods, keys: x.keys, keys_second: null, global_contract_account_id: x.global_contract_account_id ?? null, global_contract_hash: x.global_contract_hash ?? null, source: "offline illustrative fixture (fixtures/mcp/accounts.json)" } : null;
  }

  private extra: Json | null = null;

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

  nowNs(): bigint {
    const a = [...this.agents.values()][0] as Json;
    return BigInt(a?.scan_input?.now_ns ?? 0);
  }

  async holdings(id: string, account: string): Promise<Json> {
    const a = this.get(id) as Json;
    return structuredClone(a.holdings?.[account] ?? { agent: id, investor: account, holdings: [], claimable: "0" });
  }

  async buyShare(id: string, investor: string, seen: ShareTerms, part: number, amount: string): Promise<ShareOutcome> {
    const rec = ((this.world.recorded_share_buys as Json[]) ?? []).find(
      (b) => b.agent === id && b.investor === investor && b.offer_id === seen.offer_id && b.part === part && b.amount === amount,
    );
    if (!rec) {
      throw new ToolError("OFFLINE_NO_RECORDING", "offline mode replays recorded contract results only; this share buy (agent, investor, offer, part, amount) was not recorded", {
        recorded: ((this.world.recorded_share_buys as Json[]) ?? []).map((b) => ({ agent: b.agent, investor: b.investor, offer_id: b.offer_id, part: b.part, amount: b.amount })),
      });
    }
    this.agents.set(id, structuredClone(rec.after));
    return { cost: String(rec.share_bought.cost), protocol_fee: String(rec.share_bought.protocol_fee ?? "0"), refund: String(rec.refund), recorded: true };
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

/** Waits before the 1st and 2nd retry of a rate-limited (429) RPC call. Tests may shorten it. */
export const RETRY_429_MS = [1_000, 3_000];

export class TestnetBackend implements Backend {
  network: Network = "testnet";
  constructor(
    private cfg: Config,
    private fetchImpl: typeof fetch = fetch,
    /** AGENT_EXIT_PRIVATE_KEY: signs buy_agent; held here only, never logged. */
    private secret: string | null = null,
  ) {}

  /** POST with up to two retries on HTTP 429 (rate limit), waiting 1 s then
   * 3 s. A send_tx retry is safe: the same signed transaction has one hash. */
  private async post(url: string, body: string, timeoutMs: number): Promise<Response> {
    for (let attempt = 0; ; attempt++) {
      const res = await this.fetchImpl(url, {
        method: "POST",
        headers: { "content-type": "application/json" },
        body,
        // L5: a redirect could send a read to a host that is not allowlisted.
        redirect: "error",
        signal: AbortSignal.timeout(timeoutMs),
      });
      // 2026-11-07: a busy public node also answers 502/503/504.
      const busy = res.status === 429 || (res.status >= 500 && res.status <= 504);
      if (!busy || attempt >= 2) return res;
      await new Promise((r) => setTimeout(r, RETRY_429_MS[attempt]));
    }
  }

  /** A JSON-RPC call other than `query` (send_tx); errors keep the body short. */
  private async rpcMethod(url: string, method: string, params: Json, timeoutMs: number): Promise<Json> {
    const res = await this.post(url, JSON.stringify({ jsonrpc: "2.0", id: "agent-exit", method, params }), timeoutMs);
    if (!res.ok) throw new Error(`HTTP ${res.status}`);
    const body = await res.json();
    if (body.error) throw new Error(JSON.stringify(body.error).slice(0, 400));
    return body.result;
  }

  private async rpc(url: string, params: Json): Promise<Json> {
    let res: Response;
    try {
      res = await this.post(url, JSON.stringify({ jsonrpc: "2.0", id: "agent-exit", method: "query", params }), this.cfg.rpcTimeoutMs);
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

  /** A contract view from the first provider, or the second if the first
   * keeps failing (RR2). Key lists the scan compares are read per provider. */
  private async view(id: string, method: string, args: Json = {}): Promise<Json> {
    try {
      return await this.viewAt(this.cfg.rpc[0], id, method, args);
    } catch (e) {
      if (!(e instanceof ToolError) || e.code !== "RPC_ERROR" || !/: HTTP \d+$|unreachable/.test(e.message)) throw e;
      return this.viewAt(this.cfg.rpc[1], id, method, args);
    }
  }

  private async viewAt(url: string, id: string, method: string, args: Json = {}): Promise<Json> {
    const r = await this.rpc(url, {
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
    // Implicit accounts (64 hex, or 0x + 40 hex) exist on every network; on
    // this testnet RPC they are testnet accounts. Named ones must be .testnet.
    if (!id.endsWith(".testnet") && !/^([0-9a-f]{64}|0x[0-9a-f]{40})$/.test(id)) throw new ToolError("NETWORK_REFUSED", `${id} is neither a .testnet account nor an implicit account`);
  }

  async accountFacts(id: string): Promise<AccountFacts | null> {
    this.known(id);
    let account: Json;
    try {
      account = await this.rpc(this.cfg.rpc[0], { request_type: "view_account", finality: "final", account_id: id });
    } catch (e) {
      if (/UNKNOWN_ACCOUNT|does not exist/i.test(String((e as Error).message))) return null;
      throw e;
    }
    const [keys, second, account2] = await Promise.all([
      this.keys(this.cfg.rpc[0], id),
      this.keys(this.cfg.rpc[1], id).catch(() => null),
      this.rpc(this.cfg.rpc[1], { request_type: "view_account", finality: "final", account_id: id }).catch(() => null),
    ]);
    let methods: string[] = [];
    if (account.code_hash !== "11111111111111111111111111111111") {
      const code = await this.rpc(this.cfg.rpc[0], { request_type: "view_code", finality: "final", account_id: id });
      methods = wasmExports(Buffer.from(code.code_base64, "base64"));
    }
    const gid = account.global_contract_account_id ? String(account.global_contract_account_id) : null;
    const ghash = account.global_contract_hash ? String(account.global_contract_hash) : null;
    return { code_hash: String(account.code_hash), code_hash_second: account2 ? String(account2.code_hash) : null, methods, keys, keys_second: second, global_contract_account_id: gid, global_contract_hash: ghash, source: `testnet RPC (${this.cfg.rpc.join(", ")})` };
  }

  async agent(id: string): Promise<AgentView> {
    this.known(id);
    const [status, earnings, listing, limits, sales, keys, shares] = await Promise.all([
      this.view(id, "get_status"),
      this.view(id, "get_earnings"),
      this.view(id, "get_listing"),
      this.view(id, "get_limits"),
      this.view(id, "get_sales"),
      this.keys(this.cfg.rpc[0], id).catch((e) => {
        if (!(e instanceof ToolError) || e.code !== "RPC_ERROR") throw e;
        return this.keys(this.cfg.rpc[1], id); // RR2: not the scan's comparison
      }),
      this.view(id, "get_shares"),
    ]);
    return { status, earnings, listing, limits, sales, access_keys: keys, key_owners: {}, shares: Array.isArray(shares) ? shares : [] };
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
    // Review 1 L1: the Base authorization window comes from the same view,
    // so the scan's open_base_auth rule can fire through MCP too.
    const until = /^[0-9]{1,20}$/.test(String(w.open_auth_until ?? "")) ? Number(w.open_auth_until) : 0;
    return { address: typeof w.address === "string" ? w.address : null, path: String(w.path ?? ""), mpc_public_key: mpc, open_auth_until: until };
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
    // Review 5, M1: the hash handed to the scanner is the hash of the bytes we
    // parsed for the method list, and both providers must report it. Any
    // disagreement becomes a hash that matches nothing, so the scan blocks
    // with code_hash_mismatch instead of trusting one node.
    const bytes = Buffer.from(code.code_base64, "base64");
    const local = b58encode(createHash("sha256").update(bytes).digest());
    let second: string | null = null;
    try {
      second = (await this.rpc(this.cfg.rpc[1], { request_type: "view_account", finality: "final", account_id: id })).code_hash;
    } catch {
      second = null;
    }
    // Review 5, C1 (scenario C21): what the contract says it holds must be
    // backed by the token balance, read from both providers. A value that
    // cannot be read or does not agree becomes a non-number, which blocks.
    // The published token only, never one the agent names (as BW4 for the MPC signer).
    const token = this.cfg.expectedToken ?? "xusd.testnet";
    const balanceOf = async (url: string): Promise<string | null> => {
      try {
        const r = await this.rpc(url, {
          request_type: "call_function",
          finality: "final",
          account_id: token,
          method_name: "ft_balance_of",
          args_base64: Buffer.from(JSON.stringify({ account_id: id })).toString("base64"),
        });
        const v = JSON.parse(Buffer.from(r.result).toString("utf8"));
        return typeof v === "string" && /^[0-9]{1,39}$/.test(v) ? v : null;
      } catch {
        return null;
      }
    };
    const [bal1, bal2] = token ? await Promise.all([balanceOf(this.cfg.rpc[0]), balanceOf(this.cfg.rpc[1])]) : [null, null];
    const tokenBalance = bal1 !== null && bal1 === bal2 ? bal1 : "unreadable";
    const held = typeof status.held === "string" && /^[0-9]{1,39}$/.test(status.held) ? status.held : "not shown";
    const codeHash = account.code_hash === local && second === local ? local : `unverified: rpc says ${String(account.code_hash).slice(0, 44)}, second rpc ${String(second).slice(0, 44)}, bytes hash ${local}`;
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
        code_hash: codeHash,
        methods: wasmExports(bytes),
        keys: keys1,
        keys_second_provider: keys2,
        // NEAR RPC cannot list sub-accounts (an indexer can): the scan's
        // warn-only subaccount_does_not_move rule fires offline only (N11).
        subaccounts: [],
        token_balance: tokenBalance,
      },
      contract: {
        status: status.status,
        owner: status.owner,
        listing_version: status.listing_version,
        known_keys: status.known_keys,
        known_keys_hash: status.known_keys_hash,
        open_approvals: status.open_approvals,
        open_auth_until: baseWallet?.open_auth_until ?? 0,
        frozen: status.frozen,
        admin: status.admin,
        token: status.token,
        paid_to_total: status.paid_to_total,
        near_storage_solvent: status.near_storage?.solvent,
        held,
        ...(typeof status.solvent === "boolean" ? { ledger_solvent: status.solvent } : { ledger_solvent: false }),
        ...(baseWallet ? { base_wallet: { address: baseWallet.address, path: baseWallet.path, mpc_public_key: baseWallet.mpc_public_key } } : {}),
      },
      ...(listing ? { listing: listingView(listing) } : {}),
      ...(this.cfg.factory ? { born: await this.birth(id, this.cfg.factory) } : {}),
    }, expect);
  }

  /** The born-clean factory's record of `id` (null = not born there). An
   * unreadable record is passed on as a record that matches nothing, so the
   * scan blocks instead of assuming. */
  private async birth(id: string, factory: string): Promise<Json> {
    if (!id.endsWith(`.${factory}`)) return { factory, record: null };
    try {
      const r = await this.view(factory, "get_agent", { account_id: id });
      return { factory, record: r ? { agent: String(r.agent), code_hash: String(r.code_hash), state: String(r.state), owner_at_birth: String(r.owner_at_birth) } : null };
    } catch {
      return { factory, record: { agent: id, code_hash: "unreadable", state: "unreadable" } };
    }
  }

  /** One ft_transfer_call on the token, signed with the server's key. The
   * price, listing version and key hash are the ones the scan saw; the
   * contract refuses anything else and the token refunds it. */
  /** One ft_transfer_call on the published token, signed with the server's
   * key and waited on to FINAL. `what` names the call in errors. */
  private async ftTransferCall(id: string, signer: string, amount: string, msg: string, what: string): Promise<{ result: Json; txHash: string; explorer: string }> {
    this.known(id);
    const token = this.cfg.expectedToken;
    if (!token) throw new ToolError("WRITE_DISABLED", `testnet ${what} needs AGENT_EXIT_TOKEN (the published xUSD-FAKE token); nothing was sent`);
    return this.send(token, signer, "ft_transfer_call", { receiver_id: id, amount, msg }, 1n, what);
  }

  /** One signed FunctionCall from `signer` to `receiver`, sent and awaited. */
  private async send(receiver: string, signer: string, methodName: string, args: Json, deposit: bigint, what: string): Promise<{ result: Json; txHash: string; explorer: string }> {
    this.known(signer);
    if (!this.secret) throw new ToolError("WRITE_DISABLED", "testnet writes need AGENT_EXIT_PRIVATE_KEY in the server env; nothing was sent");
    let key: NearKey;
    try {
      key = new NearKey(this.secret);
    } catch (e) {
      throw new ToolError("WRITE_DISABLED", `AGENT_EXIT_PRIVATE_KEY: ${(e as Error).message}; nothing was sent`);
    }
    if (this.cfg.signerPk && this.cfg.signerPk !== key.publicKeyString) {
      throw new ToolError("WRITE_DISABLED", "AGENT_EXIT_PRIVATE_KEY does not match AGENT_EXIT_SIGNER_PK; nothing was sent");
    }
    const ak = await this.rpc(this.cfg.rpc[0], {
      request_type: "view_access_key",
      finality: "final",
      account_id: signer,
      public_key: key.publicKeyString,
    });
    const { signed, hash: txHash } = signTx(
      {
        signerId: signer,
        publicKey: key.publicKey,
        nonce: BigInt(ak.nonce) + 1n,
        receiverId: receiver,
        blockHash: b58(ak.block_hash),
        methodName,
        args: Buffer.from(JSON.stringify(args)),
        gas: 300_000_000_000_000n,
        deposit,
      },
      key,
    );
    const explorer = `https://testnet.nearblocks.io/txns/${txHash}`;
    let result: Json;
    try {
      result = await this.rpcMethod(this.cfg.rpc[0], "send_tx", { signed_tx_base64: signed.toString("base64"), wait_until: "FINAL" }, 90_000);
    } catch (e) {
      // Sent or not, we cannot tell: never report it as not sent.
      throw new ToolError("SENT_UNCONFIRMED", `the ${what} transaction ${txHash} may have been sent (${(e as Error).message}); check ${explorer} before retrying`, { tx_hash: txHash, explorer });
    }
    return { result, txHash, explorer };
  }

  async buy(id: string, buyer: string, pk: string, amount: string, version: number, hash: string, deliveryPk?: string): Promise<BuyOutcome> {
    const msg = JSON.stringify({ kind: "buy", listing_version: version, known_keys_hash: hash, buyer_runner_pk: pk, ...(deliveryPk ? { delivery_pk: deliveryPk } : {}) });
    const { result, txHash, explorer } = await this.ftTransferCall(id, buyer, amount, msg, "buy");
    const sale = txEvents(result, "sale")[0];
    if (!sale) {
      const rolled = txEvents(result, "sale_rolled_back")[0];
      const failure = result?.status?.Failure ? JSON.stringify(result.status.Failure).slice(0, 300) : null;
      throw new ToolError("BUY_REFUSED", `no sale: ${rolled ? "the handover was rolled back and the payment refunded" : failure ?? "the contract refused the buy; the token refunded the payment"} (${explorer})`, {
        tx_hash: txHash,
        explorer,
      });
    }
    const settling = txEvents(result, "settling")[0] ?? {};
    const key_batch = [
      ...((settling.delete_keys as string[]) ?? []).map((k) => ({ action: "delete", key: k })),
      ...(settling.add_key ? [{ action: "add_fc", key: String(settling.add_key) }] : []),
    ];
    return { sale, key_batch, overpay_refund: String(sale.overpay_refund ?? "0"), recorded: false, tx_hash: txHash, explorer };
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

  nowNs(): bigint {
    return BigInt(Date.now()) * 1_000_000n;
  }

  async txOutcome(hash: string, sender: string): Promise<TxOutcome> {
    if (!/^[1-9A-HJ-NP-Za-km-z]{43,44}$/.test(hash)) throw new ToolError("BAD_INPUT", "not a transaction hash");
    if (!sender.endsWith(".testnet")) throw new ToolError("NETWORK_REFUSED", `${sender} is not a .testnet account`);
    let last = "";
    for (const url of ARCHIVAL_RPC) {
      try {
        return txOutcomeOf(await this.rpcMethod(url, "tx", { tx_hash: hash, sender_account_id: sender, wait_until: "NONE" }, this.cfg.rpcTimeoutMs));
      } catch (e) {
        last = (e as Error).message;
      }
    }
    throw new ToolError("RPC_ERROR", `transaction ${hash.slice(0, 8)}… not read from the archival RPCs: ${last.slice(0, 160)}`);
  }

  async holdings(id: string, account: string): Promise<Json> {
    this.known(id);
    return this.view(id, "get_holdings", { account_id: account });
  }

  async sealedConfig(id: string, saleId: number): Promise<Json | null> {
    this.known(id);
    return this.view(id, "get_sealed_config", { sale_id: saleId });
  }

  async preview(id: string): Promise<Json | null> {
    this.known(id);
    return this.view(id, "get_preview");
  }

  async sealedCall(id: string, signer: string, method: "release_upfront" | "dispute_config" | "request_preview", args: Json): Promise<SealedCallOutcome> {
    this.known(id);
    const { result, txHash, explorer } = await this.send(id, signer, method, args, 0n, method);
    const ok = result?.status?.SuccessValue;
    if (ok === undefined) {
      const failure = result?.status?.Failure ? JSON.stringify(result.status.Failure).slice(0, 300) : "no outcome";
      throw new ToolError("CONTRACT_REFUSED", `${method} failed: ${failure} (${explorer})`, { tx_hash: txHash, explorer });
    }
    let value: Json = {};
    try {
      value = JSON.parse(Buffer.from(String(ok), "base64").toString("utf8"));
    } catch {
      value = {};
    }
    return { value, recorded: false, tx_hash: txHash, explorer };
  }

  async buyShare(id: string, investor: string, seen: ShareTerms, part: number, amount: string): Promise<ShareOutcome> {
    const msg = JSON.stringify({ kind: "buy_share", offer_id: seen.offer_id, bps: seen.bps, price: seen.price, end: seen.end, part });
    const { result, txHash, explorer } = await this.ftTransferCall(id, investor, amount, msg, "share buy");
    const bought = txEvents(result, "share_bought")[0];
    if (!bought) {
      const refused = txEvents(result, "refused")[0];
      const failure = result?.status?.Failure ? JSON.stringify(result.status.Failure).slice(0, 300) : null;
      throw new ToolError("SHARE_REFUSED", `no share bought: ${refused ? `the contract refused it (${String(refused.reason)}); the token refunded the payment` : failure ?? "the contract refused it; the token refunded the payment"} (${explorer})`, {
        tx_hash: txHash,
        explorer,
      });
    }
    const cost = String(bought.cost);
    return { cost, protocol_fee: String(bought.protocol_fee ?? "0"), refund: (BigInt(amount) - BigInt(cost)).toString(), recorded: false, tx_hash: txHash, explorer };
  }
}

export function makeBackend(cfg: Config, env: NodeJS.ProcessEnv = process.env): Backend {
  return cfg.network === "testnet" ? new TestnetBackend(cfg, fetch, env.AGENT_EXIT_PRIVATE_KEY?.trim() || null) : new OfflineBackend(cfg.worldPath);
}

function b58(s: string): Buffer {
  const ALPHABET = "123456789ABCDEFGHJKLMNPQRSTUVWXYZabcdefghijkmnopqrstuvwxyz";
  let n = 0n;
  for (const c of s) n = n * 58n + BigInt(ALPHABET.indexOf(c));
  const out = Buffer.alloc(32);
  for (let i = 31; i >= 0; i--) {
    out[i] = Number(n & 0xffn);
    n >>= 8n;
  }
  return out;
}
