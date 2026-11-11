// A small MCP client over stdio, used by the tests and the scripted
// "agent buys agent" run. It talks to the real server process.
import { Client } from "@modelcontextprotocol/sdk/client/index.js";
import { StdioClientTransport } from "@modelcontextprotocol/sdk/client/stdio.js";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import type { Json } from "./backend.js";

export const REPO = resolve(dirname(fileURLToPath(import.meta.url)), "..", "..", "..");
export const SERVER = join(REPO, "mcp", "dist", "src", "server.js");

export interface Session {
  client: Client;
  call(name: string, args?: Json): Promise<{ ok: boolean; body: Json }>;
  close(): Promise<void>;
  stderr: () => string;
}

/** Setup variables passed through from the caller's environment: where the
 * scan binary is (AGENT_EXIT_SCAN_BIN, CARGO_TARGET_DIR) and how to reach the
 * network (proxy, extra CA). Never keys or account settings. */
export const PASS_THROUGH_ENV = [
  "AGENT_EXIT_SCAN_BIN",
  "CARGO_TARGET_DIR",
  "HTTPS_PROXY",
  "HTTP_PROXY",
  "NO_PROXY",
  "https_proxy",
  "http_proxy",
  "no_proxy",
  "NODE_EXTRA_CA_CERTS",
  "NODE_USE_ENV_PROXY",
] as const;

/** The pass-through subset of `from` (default process.env). */
export function passThroughEnv(from: NodeJS.ProcessEnv = process.env): Record<string, string> {
  const out: Record<string, string> = {};
  for (const k of PASS_THROUGH_ENV) if (from[k]) out[k] = from[k] as string;
  return out;
}

/** Start the server with `env` plus PATH and the setup variables above, and connect. */
export async function connect(env: Record<string, string>): Promise<Session> {
  const transport = new StdioClientTransport({
    command: process.execPath,
    args: [SERVER],
    env: { PATH: process.env.PATH ?? "", AGENT_EXIT_REPO: REPO, ...passThroughEnv(), ...env },
    stderr: "pipe",
  });
  let err = "";
  transport.stderr?.on("data", (d: Buffer) => (err += d.toString()));
  const client = new Client({ name: "agent-exit-test-client", version: "0.1.0" });
  await client.connect(transport);
  return {
    client,
    async call(name, args = {}) {
      // check_claims re-reads every recorded testnet transaction from archival
      // nodes; under rate limits that takes longer than the SDK's 60 s default.
      const r = await client.callTool({ name, arguments: args }, undefined, { timeout: 300_000 });
      const content = r.content as { type: string; text: string }[];
      const body = JSON.parse(content[0].text);
      return { ok: !r.isError, body };
    },
    close: () => client.close(),
    stderr: () => err,
  };
}
