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

/** Start the server with exactly `env` (plus PATH) and connect. */
export async function connect(env: Record<string, string>): Promise<Session> {
  const transport = new StdioClientTransport({
    command: process.execPath,
    args: [SERVER],
    env: { PATH: process.env.PATH ?? "", AGENT_EXIT_REPO: REPO, ...env },
    stderr: "pipe",
  });
  let err = "";
  transport.stderr?.on("data", (d: Buffer) => (err += d.toString()));
  const client = new Client({ name: "agent-exit-test-client", version: "0.1.0" });
  await client.connect(transport);
  return {
    client,
    async call(name, args = {}) {
      const r = await client.callTool({ name, arguments: args });
      const content = r.content as { type: string; text: string }[];
      const body = JSON.parse(content[0].text);
      return { ok: !r.isError, body };
    },
    close: () => client.close(),
    stderr: () => err,
  };
}
