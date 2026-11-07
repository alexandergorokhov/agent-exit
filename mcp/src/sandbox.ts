// The live sandbox (tool `sandbox`): the real contract state machine
// (contracts/agent-core) and the buyer's scan (tools/scan), compiled to wasm
// from this repo (tools/playground, `make playground-wasm`), the same file
// the demo page runs. Any action, in any order, as the seller, buyer,
// investor or a customer; the answer is what the contract code returned.
// In-memory only: it never touches a chain and never sends money.
import { readFileSync } from "node:fs";
import { join } from "node:path";
import { type Json, ToolError } from "./backend.js";

export const SANDBOX_ACTIONS = [
  "reset", "view",
  "pay_customer", "pay_verified", "pay_self", "pay_sub", "pay_fresh",
  "plant_key", "remove_key", "toggle_upgrade",
  "list", "cancel", "scan", "buy",
  "withdraw", "relist", "set_key", "old_key", "hidden_key", "hidden_redeploy",
  "wait", "offer_share", "buy_share", "claim_share", "end_share",
  "commit_config", "deliver_config", "deliver_fake", "open_config", "dispute_config", "claim_undelivered",
] as const;

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
