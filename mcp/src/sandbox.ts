// The live sandbox (tool `sandbox`): the real contract state machine
// (contracts/agent-core) and the buyer's scan (tools/scan), compiled to wasm
// from this repo (tools/playground, `make playground-wasm`), the same file
// the demo page runs. Any action, in any order, as the seller, buyer,
// investor or a customer; the answer is what the contract code returned.
// In-memory only: it never touches a chain and never sends money.
import { readFileSync } from "node:fs";
import { join } from "node:path";
import { type Json, ToolError } from "./backend.js";

/** One sandbox action: who acts, its optional args, what it does. The input
 * schema's enum, `{action: "help"}` and the docs come from this table. */
export interface SandboxAction {
  who: string;
  args: Record<string, string>;
  does: string;
}

const NONE: Record<string, string> = {};

/** Every sandbox action, in the order `help` lists them. */
export const SANDBOX_ACTION_TABLE = {
  reset: { who: "page", args: NONE, does: "a new world: alice owns translator, 3 real customers paid it; step 0" },
  view: { who: "page", args: NONE, does: "the current state; changes nothing" },
  pay_customer: { who: "a customer", args: NONE, does: "a stranger pays 25: counted as real revenue" },
  pay_verified: { who: "a customer", args: NONE, does: "a stranger pays with a NEAR AI signature" },
  pay_self: { who: "the owner", args: NONE, does: "the owner pays their own agent 200: flagged, not counted" },
  pay_sub: { who: "bot.<owner>", args: NONE, does: "a sub-account of the owner pays: flagged" },
  pay_fresh: { who: "the seller", args: NONE, does: "the seller pays 200 from a wallet nothing links to them" },
  plant_key: { who: "alice", args: NONE, does: "deploys the agent on a plain account of her own (not through the factory) and keeps a hidden full-access key the contract never sees" },
  remove_key: { who: "alice", args: NONE, does: "removes the hidden key" },
  toggle_upgrade: { who: "alice", args: NONE, does: "adds or removes an upgrade method" },
  list: {
    who: "alice",
    args: { price: "price in xUSD-FAKE dollars (default 1000)", pay_from_revenue: "true: hold the whole price; the seller is paid only by customer payments after the sale" },
    does: "list_for_sale",
  },
  cancel: { who: "alice", args: NONE, does: "cancels the listing" },
  scan: { who: "bob", args: NONE, does: "the buyer's scan of the listed agent" },
  buy: { who: "bob", args: { skip_scan: "true: buy without a clean scan (the real SDK and MCP buy_agent never do)" }, does: "buys in one transaction: owner, keys and money move together" },
  withdraw: { who: "alice", args: NONE, does: "after a sale: the old seller tries to withdraw" },
  relist: { who: "alice", args: NONE, does: "after a sale: the old seller tries to list again" },
  set_key: { who: "alice", args: NONE, does: "after a sale: the old seller tries to add a key" },
  old_key: { who: "alice", args: NONE, does: "after a sale: the old seller tries her old key" },
  hidden_key: { who: "alice", args: NONE, does: "runs the agent with the hidden key" },
  hidden_redeploy: { who: "alice", args: NONE, does: "redeploys her own code with the hidden key (nobody paid: the contract refused to list or sell the agent)" },
  wait: { who: "clock", args: { days: "days that pass (default 30)" }, does: "time passes; the earn-out closes" },
  offer_share: { who: "alice", args: NONE, does: "offers 20% of every payment for 500" },
  buy_share: { who: "dave", args: NONE, does: "buys the share: 245.00 to alice now after the 1% fee, 250.00 held" },
  claim_share: { who: "dave", args: NONE, does: "claims what the share earned" },
  end_share: { who: "alice", args: NONE, does: "ends the share slice" },
  commit_config: { who: "alice", args: NONE, does: "commits to the sealed prompt bundle before listing" },
  deliver_config: { who: "alice", args: NONE, does: "delivers the committed bundle sealed to bob's one-time key" },
  deliver_fake: { who: "alice", args: NONE, does: "delivers a different prompt instead" },
  open_config: { who: "bob", args: NONE, does: "decrypts the delivery and checks it against the commitment" },
  dispute_config: { who: "bob", args: NONE, does: "proves a mismatch on-chain and gets the whole price back" },
  claim_undelivered: { who: "bob", args: NONE, does: "no delivery in 2 days: the whole price back" },
  accept_config: { who: "bob", args: NONE, does: "accepts: the held upfront part goes to alice and releases start" },
} satisfies Record<string, SandboxAction>;

export type SandboxActionName = keyof typeof SANDBOX_ACTION_TABLE;
export const SANDBOX_ACTIONS = Object.keys(SANDBOX_ACTION_TABLE) as SandboxActionName[];

/** Example sequences for `help`; a test runs each one in the real wasm. */
export const SANDBOX_EXAMPLES: Record<string, { what: string; steps: ({ action: SandboxActionName } & Record<string, unknown>)[] }> = {
  invest: {
    what: "dave buys 20% of translator's revenue; a customer pays; dave claims 5.00",
    steps: [{ action: "reset" }, { action: "offer_share" }, { action: "buy_share" }, { action: "pay_customer" }, { action: "claim_share" }],
  },
  honest_sale: {
    what: "a clean scan, a one-transaction buy, alice locked out, the earn-out closes; bob owns it",
    steps: [{ action: "reset" }, { action: "pay_customer" }, { action: "list" }, { action: "scan" }, { action: "buy" }, { action: "withdraw" }, { action: "wait", days: 30 }],
  },
  cheat: {
    what: "alice fakes revenue (flagged) and deploys the agent plainly with a hidden key; the contract refuses to list it (NotBornClean), the scan blocks it, and a buy with skip_scan is refused by the contract too",
    steps: [{ action: "reset" }, { action: "pay_self" }, { action: "plant_key" }, { action: "list" }, { action: "scan" }, { action: "buy" }, { action: "buy", skip_scan: true }],
  },
};

/** The body of `{action: "help"}`: built from the table, so it cannot drift. */
export function sandboxHelp(): Json {
  return {
    actions: SANDBOX_ACTIONS.map((action) => ({ action, ...SANDBOX_ACTION_TABLE[action] })),
    examples: SANDBOX_EXAMPLES,
    note: "send one action per call, in any order; the contract decides. Try your own: e.g. plant_key, list, buy with skip_scan true: the contract refuses both (NotBornClean).",
  };
}

interface Exports {
  memory: WebAssembly.Memory;
  pg_alloc(len: number): number;
  pg_call(): number;
  pg_out(): number;
}

export class Sandbox {
  private ex: Exports | null = null;
  private steps = 0;
  constructor(private wasmPath: string) {}

  static forRepo(repo: string): Sandbox {
    return new Sandbox(join(repo, "web", "playground.wasm"));
  }

  private load(): Exports {
    if (this.ex) return this.ex;
    let bytes: Buffer;
    try {
      bytes = readFileSync(this.wasmPath);
    } catch {
      throw new ToolError("SANDBOX_UNAVAILABLE", "web/playground.wasm is missing: run make playground-wasm");
    }
    const inst = new WebAssembly.Instance(new WebAssembly.Module(new Uint8Array(bytes)), {});
    this.ex = inst.exports as unknown as Exports;
    return this.ex;
  }

  /** One action into the wasm; returns {last, view}. */
  call(action: Json): { last: Json; view: Json } {
    const ex = this.load();
    const input = new TextEncoder().encode(JSON.stringify(action));
    const p = ex.pg_alloc(input.length);
    new Uint8Array(ex.memory.buffer, p, input.length).set(input);
    const n = ex.pg_call();
    const out = JSON.parse(new TextDecoder().decode(new Uint8Array(ex.memory.buffer, ex.pg_out(), n)));
    if (!out.ok) throw new ToolError("BAD_INPUT", String(out.error).slice(0, 200));
    this.steps = action.op === "reset" ? 0 : this.steps + 1;
    return { last: out.last, view: out.view };
  }

  get step(): number {
    return this.steps;
  }
}
