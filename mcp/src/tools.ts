// The six Agent Exit tools. JSON in, JSON out; every input and output has a
// schema in schemas/mcp/ and names its schema_version.
//
// Read tools: list_agents, get_earnings, scan_agent, verify_lockout.
// Write tools: buy_agent, list_for_sale. They need AGENT_EXIT_ALLOW_WRITE=1
// and AGENT_EXIT_ACCOUNT_ID in the server env, confirm: true, and buy_agent
// needs max_price, stays inside the session spend cap, and scans first:
// any finding refuses the buy before anything is sent.
import { spawnSync } from "node:child_process";
import { readFileSync } from "node:fs";
import { join } from "node:path";
import { Ajv, type ValidateFunction } from "ajv";
import { type Backend, type Json, ToolError } from "./backend.js";
import type { Config } from "./config.js";

export const TOOL_NAMES = [
  "list_agents",
  "get_earnings",
  "scan_agent",
  "buy_agent",
  "verify_lockout",
  "list_for_sale",
] as const;
export type ToolName = (typeof TOOL_NAMES)[number];
export const WRITE_TOOLS: ToolName[] = ["buy_agent", "list_for_sale"];

const DESCRIPTIONS: Record<ToolName, string> = {
  list_agents: "List agents with owner, status, price and meter totals (counted = real customers; flagged = self, sub-account or round-trip payments). Read-only. All money is xUSD-FAKE.",
  get_earnings: "Earnings meter of one agent by payer and day: gross, counted, verified (NEAR AI signature checked on-chain), flagged. Read-only.",
  scan_agent: "Backdoor scan of an agent account before buying: extra or unknown keys, wrong code hash, upgrade methods, open approvals, listing changes. Read-only. Buy only if verdict is clean.",
  buy_agent: "Buy a listed agent in one transaction (deed moves, seller's runner key deleted, your runner key added, 70% paid now, 30% earn-out). WRITE: needs confirm: true and max_price; scans first and refuses on any finding.",
  verify_lockout: "After a sale: check that an account (the old seller) no longer owns the agent, has no key on it, and that owner-only calls are refused. Read-only.",
  list_for_sale: "List an agent you own for sale (resale). WRITE: needs confirm: true.",
};

export interface SchemaSet {
  input: Record<ToolName, Json>;
  output: Record<ToolName, Json>;
  error: Json;
  validateInput: Record<ToolName, ValidateFunction>;
  validateOutput: Record<ToolName, ValidateFunction>;
  validateError: ValidateFunction;
}

export function loadSchemas(repo: string): SchemaSet {
  const dir = join(repo, "schemas", "mcp");
  const read = (f: string): Json => JSON.parse(readFileSync(join(dir, f), "utf8"));
  const ajv = new Ajv({ allErrors: true, strict: true });
  const input = {} as Record<ToolName, Json>;
  const output = {} as Record<ToolName, Json>;
  const validateInput = {} as Record<ToolName, ValidateFunction>;
  const validateOutput = {} as Record<ToolName, ValidateFunction>;
  for (const t of TOOL_NAMES) {
    input[t] = read(`${t}.input.v1.json`);
    output[t] = read(`${t}.output.v1.json`);
    validateInput[t] = ajv.compile(input[t]);
    validateOutput[t] = ajv.compile(output[t]);
  }
  const error = read("error.v1.json");
  return { input, output, error, validateInput, validateOutput, validateError: ajv.compile(error) };
}

/** What MCP clients see in tools/list: the schema files minus `$schema`/`$id`. */
export function toolList(s: SchemaSet): Json[] {
  const strip = (x: Json): Json => {
    const { $schema: _a, $id: _b, ...rest } = x;
    return rest;
  };
  return TOOL_NAMES.map((name) => ({
    name,
    description: DESCRIPTIONS[name],
    inputSchema: strip(s.input[name]),
    outputSchema: strip(s.output[name]),
    annotations: {
      readOnlyHint: !WRITE_TOOLS.includes(name),
      destructiveHint: false,
      openWorldHint: false,
    },
  }));
}

// ------------------------------------------------------------------ money

const UNIT = 1_000_000n;

/** Base units -> "12.34" (2 decimals, rounded down). */
export function usd(units: string | number | bigint): string {
  const u = BigInt(units);
  const cents = (u % UNIT) / 10_000n;
  return `${u / UNIT}.${cents.toString().padStart(2, "0")}`;
}

/** Dollars (number, at most 2 decimals) -> base units string. */
export function units(dollars: number): string {
  const cents = Math.round(dollars * 100);
  if (Math.abs(cents / 100 - dollars) > 1e-9) throw new ToolError("BAD_INPUT", `${dollars}: at most 2 decimals`);
  return (BigInt(cents) * 10_000n).toString();
}

// ------------------------------------------------------------------ tools

export class Tools {
  private spent = 0n;

  constructor(
    private cfg: Config,
    private backend: Backend,
    public schemas: SchemaSet,
  ) {}

  private base(tool: ToolName): Json {
    return { schema_version: `agent-exit.mcp.${tool}.v1`, network: this.backend.network };
  }

  /** Validate, run, and return either a schema-valid output or an error. */
  async call(name: string, args: Json): Promise<{ ok: boolean; body: Json }> {
    try {
      if (!(TOOL_NAMES as readonly string[]).includes(name)) {
        throw new ToolError("BAD_INPUT", `unknown tool ${name}`);
      }
      const tool = name as ToolName;
      const v = this.schemas.validateInput[tool];
      if (!v(args ?? {})) throw new ToolError("BAD_INPUT", "arguments do not match the input schema", v.errors);
      const body = await this.run(tool, args ?? {});
      const vo = this.schemas.validateOutput[tool];
      if (!vo(body)) throw new Error(`internal: ${tool} output does not match its schema: ${JSON.stringify(vo.errors)}`);
      return { ok: true, body };
    } catch (e) {
      const err =
        e instanceof ToolError
          ? { code: e.code, message: e.message, ...(e.details === undefined ? {} : { details: e.details }) }
          : { code: "BAD_INPUT", message: (e as Error).message };
      return { ok: false, body: { schema_version: "agent-exit.mcp.error.v1", ok: false, error: err, sent: false } };
    }
  }

  private run(tool: ToolName, a: Json): Promise<Json> {
    switch (tool) {
      case "list_agents":
        return this.listAgents(a.status);
      case "get_earnings":
        return this.getEarnings(a.agent);
      case "scan_agent":
        return this.scanAgent(a.agent, a.expected_listing_version);
      case "buy_agent":
        return this.buyAgent(a.agent, a.max_price, a.confirm, a.buyer_runner_pk);
      case "verify_lockout":
        return this.verifyLockout(a.agent, a.account, a.account_keys);
      case "list_for_sale":
        return this.listForSale(a.agent, a.price, a.confirm);
    }
  }

  private async listAgents(status?: string): Promise<Json> {
    const out = [];
    for (const id of await this.backend.listAgents()) {
      const v = await this.backend.agent(id);
      if (status && v.status.status !== status) continue;
      const e = v.earnings;
      out.push({
        agent: id,
        owner: v.status.owner,
        status: v.status.status,
        listing_version: v.status.listing_version,
        price: v.listing ? usd(v.listing.price) : null,
        gross: usd(e.gross),
        counted: usd(e.counted),
        verified: usd(e.verified),
        flagged: usd(e.flagged),
        frozen: v.status.frozen,
      });
    }
    return { ...this.base("list_agents"), agents: out };
  }

  private async getEarnings(agent: string): Promise<Json> {
    const e = (await this.backend.agent(agent)).earnings;
    return {
      ...this.base("get_earnings"),
      agent,
      earnings: {
        gross: usd(e.gross),
        counted: usd(e.counted),
        verified: usd(e.verified),
        flagged: usd(e.flagged),
        flagged_self: usd(e.flagged_self),
        flagged_seller_subaccount: usd(e.flagged_seller_subaccount),
        flagged_round_trip: usd(e.flagged_round_trip),
        payments: e.payments,
        counted_payers: e.counted_payers,
        days: (e.days as Json[]).map((d) => ({
          day: d.day,
          gross: usd(d.gross),
          counted: usd(d.counted),
          verified: usd(d.verified),
          payments: d.payments,
        })),
      },
      meaning:
        "counted = paid by accounts that are not the owner, a past owner, their sub-accounts or a round trip; verified = counted payments whose NEAR AI signature the contract checked; flagged payments are shown but never counted",
    };
  }

  /** Run the Rust scanner (tools/scan, one source of truth for the rules). */
  private async scan(agent: string, expectedVersion?: number): Promise<Json> {
    const input = await this.backend.scanInput(agent, expectedVersion);
    if (!this.cfg.scanBin) {
      throw new ToolError("SCAN_UNAVAILABLE", "scan binary not found: run `cargo build -p scan` (or set AGENT_EXIT_SCAN_BIN); buying is refused without a scan");
    }
    const r = spawnSync(this.cfg.scanBin, ["-"], { input: JSON.stringify(input), encoding: "utf8" });
    if (r.status === null || ![0, 2, 3].includes(r.status)) {
      throw new ToolError("SCAN_UNAVAILABLE", `scan failed (exit ${r.status}): ${r.stderr}`);
    }
    return JSON.parse(r.stdout);
  }

  private async scanAgent(agent: string, expectedVersion?: number): Promise<Json> {
    const r = await this.scan(agent, expectedVersion);
    return {
      ...this.base("scan_agent"),
      agent,
      verdict: r.verdict,
      safe_to_buy: r.verdict === "clean",
      findings: r.findings,
      buy_args: r.buy_args ?? null,
      scanner: "tools/scan (scan-report.v1)",
    };
  }

  private writeGate(): string {
    if (!this.cfg.allowWrite || !this.cfg.accountId) {
      throw new ToolError(
        "WRITE_DISABLED",
        "write tools are off: start the server with AGENT_EXIT_ALLOW_WRITE=1 and AGENT_EXIT_ACCOUNT_ID (testnet also needs AGENT_EXIT_PRIVATE_KEY)",
      );
    }
    if (this.backend.network === "testnet" && !this.cfg.hasPrivateKey) {
      throw new ToolError("WRITE_DISABLED", "testnet writes need AGENT_EXIT_PRIVATE_KEY in the server env");
    }
    return this.cfg.accountId;
  }

  private async buyAgent(agent: string, maxPrice: number, confirm: boolean, pk: string): Promise<Json> {
    const buyer = this.writeGate();
    if (confirm !== true) throw new ToolError("CONFIRM_REQUIRED", "buy_agent needs confirm: true");
    const v = await this.backend.agent(agent);
    if (v.status.status !== "Listed" || !v.listing) throw new ToolError("NOT_LISTED", `${agent} is ${v.status.status}, not listed`);
    const price = BigInt(v.listing.price);
    if (price > BigInt(units(maxPrice))) {
      throw new ToolError("OVER_MAX_PRICE", `price ${usd(price)} > max_price ${maxPrice.toFixed(2)}`, { price: usd(price) });
    }
    const cap = BigInt(units(this.cfg.sessionCap));
    if (this.spent + price > cap) {
      throw new ToolError("SESSION_CAP", `session spend would be ${usd(this.spent + price)} > cap ${usd(cap)} (AGENT_EXIT_SESSION_CAP)`);
    }
    const r = await this.scan(agent);
    if (r.verdict !== "clean") {
      throw new ToolError("SCAN_BLOCKED", `scan verdict ${r.verdict}: not buying`, {
        findings: (r.findings as Json[]).filter((f) => f.severity === "block"),
      });
    }
    const { listing_version, known_keys_hash } = r.buy_args;
    const out = await this.backend.buy(agent, buyer, pk, price.toString(), listing_version, known_keys_hash);
    this.spent += price;
    const after = await this.backend.agent(agent);
    const s = out.sale;
    return {
      ...this.base("buy_agent"),
      agent,
      buyer,
      seller: s.seller,
      sale_id: s.sale_id,
      price: usd(s.price),
      paid_now: usd(s.paid_now),
      escrow: usd(s.escrow),
      overpay_refund: usd(out.overpay_refund),
      listing_version,
      known_keys_hash,
      scan_verdict: "clean",
      key_batch: out.key_batch,
      owner_after: after.status.owner,
      session_spent: usd(this.spent),
      recorded: out.recorded,
    };
  }

  private async verifyLockout(agent: string, account: string, accountKeys?: string[]): Promise<Json> {
    const v = await this.backend.agent(agent);
    const keys =
      accountKeys ?? Object.entries(v.key_owners).filter(([, who]) => who === account).map(([k]) => k);
    const onChain = new Set((v.access_keys as Json[]).map((k) => k.public_key));
    const known = new Set(v.status.known_keys as string[]);
    const full = (v.access_keys as Json[]).filter((k) => k.permission === "FullAccess").map((k) => k.public_key);
    const checks = [
      { id: "not_owner", pass: v.status.owner !== account, detail: `deed owner is ${v.status.owner}` },
      {
        id: "no_key_on_account",
        pass: keys.every((k) => !onChain.has(k) && !known.has(k)),
        detail: keys.length
          ? `${keys.length} key(s) of ${account} checked against the account's key list and the contract's known keys`
          : `no key of ${account} known; pass account_keys to check specific keys`,
      },
      {
        id: "no_full_access_key",
        pass: full.length === 0,
        detail: full.length ? `full-access key(s) on the account: ${full.join(", ")}` : "the account has no full-access key",
      },
    ];
    const probes = await this.backend.probes(agent, account);
    if (probes) {
      for (const [method, result] of Object.entries(probes)) {
        checks.push({ id: `refused_${method}`, pass: result === "NotOwner", detail: `${method} by ${account}: ${result}` });
      }
    }
    return {
      ...this.base("verify_lockout"),
      agent,
      account,
      locked_out: checks.every((c) => c.pass),
      checks,
    };
  }

  private async listForSale(agent: string, price: number, confirm: boolean): Promise<Json> {
    const seller = this.writeGate();
    if (confirm !== true) throw new ToolError("CONFIRM_REQUIRED", "list_for_sale needs confirm: true");
    const v = await this.backend.agent(agent);
    if (v.status.owner !== seller) throw new ToolError("NOT_OWNER", `${seller} does not own ${agent} (owner: ${v.status.owner})`);
    const r = await this.backend.list(agent, seller, units(price));
    const l = r.listing;
    return {
      ...this.base("list_for_sale"),
      agent,
      seller: l.seller,
      price: usd(l.price),
      paid_now: usd(l.paid_now),
      escrow: usd(l.escrow),
      listing_version: l.listing_version,
      known_keys_hash: l.known_keys_hash,
      recorded: r.recorded,
    };
  }
}
