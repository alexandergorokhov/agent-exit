import { readFileSync } from "node:fs";
import { join } from "node:path";
import { connect, REPO, type Session } from "../src/client.js";
import { loadSchemas } from "../src/tools.js";

export { REPO };
export const schemas = loadSchemas(REPO);
export const world = JSON.parse(readFileSync(join(REPO, "fixtures", "mcp", "world.json"), "utf8"));
export const RUNNER_PK: string = world.recorded_buys[0].buyer_runner_pk;

/** A server with writes on, acting as `account`. */
export function writer(account = "summarizer.testnet", extra: Record<string, string> = {}): Promise<Session> {
  return connect({ AGENT_EXIT_ALLOW_WRITE: "1", AGENT_EXIT_ACCOUNT_ID: account, AGENT_EXIT_RUNNER_PK: RUNNER_PK, ...extra });
}

/** Assert the body matches the tool's output schema (or error.v1). */
export function valid(tool: keyof typeof schemas.validateOutput, r: { ok: boolean; body: unknown }): boolean {
  const v = r.ok ? schemas.validateOutput[tool] : schemas.validateError;
  if (!v(r.body)) throw new Error(`${tool}: ${JSON.stringify(v.errors)}`);
  return true;
}
