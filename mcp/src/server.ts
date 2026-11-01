#!/usr/bin/env node
// Agent Exit MCP server (stdio). Offline by default (recorded fixtures);
// AGENT_EXIT_NETWORK=testnet for read-only testnet views. Never mainnet.
// Read-only unless AGENT_EXIT_ALLOW_WRITE=1 and AGENT_EXIT_ACCOUNT_ID are set.
import { Server } from "@modelcontextprotocol/sdk/server/index.js";
import { StdioServerTransport } from "@modelcontextprotocol/sdk/server/stdio.js";
import { CallToolRequestSchema, ListToolsRequestSchema } from "@modelcontextprotocol/sdk/types.js";
import { makeBackend } from "./backend.js";
import { ConfigError, loadConfig } from "./config.js";
import { loadSchemas, toolList, Tools } from "./tools.js";

export const SERVER_NAME = "agent-exit";
export const SERVER_VERSION = "0.1.0";

async function main(): Promise<void> {
  let cfg;
  try {
    cfg = loadConfig();
  } catch (e) {
    if (e instanceof ConfigError) {
      process.stderr.write(`agent-exit-mcp: ${e.message}\n`);
      process.exit(2);
    }
    throw e;
  }
  const schemas = loadSchemas(cfg.repo);
  const tools = new Tools(cfg, makeBackend(cfg), schemas);
  // The low-level Server serves the JSON schemas in schemas/mcp/ verbatim.
  const server = new Server(
    { name: SERVER_NAME, version: SERVER_VERSION },
    {
      capabilities: { tools: {} },
      instructions:
        "Agent Exit: buy AI agents with proven earnings and a proven handover. Always scan_agent before buy_agent; buy only on verdict clean; after buying, verify_lockout the seller. All money is xUSD-FAKE.",
    },
  );
  server.setRequestHandler(ListToolsRequestSchema, async () => ({ tools: toolList(schemas) }));
  server.setRequestHandler(CallToolRequestSchema, async (req) => {
    const r = await tools.call(req.params.name, req.params.arguments ?? {});
    const text = JSON.stringify(r.body, null, 2);
    // Errors carry no structuredContent (it would be checked against the
    // tool's success schema); the text is error.v1 JSON.
    return r.ok
      ? { content: [{ type: "text", text }], structuredContent: r.body }
      : { content: [{ type: "text", text }], isError: true };
  });
  process.stderr.write(
    `agent-exit-mcp: network=${cfg.network} writes=${cfg.allowWrite && cfg.accountId ? `on as ${cfg.accountId}` : "off"} scan=${cfg.scanBin ?? "missing"}\n`,
  );
  await server.connect(new StdioServerTransport());
}

main().catch((e) => {
  process.stderr.write(`agent-exit-mcp: ${(e as Error).stack ?? e}\n`);
  process.exit(1);
});
