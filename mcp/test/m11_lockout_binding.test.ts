// M11 (review 5, M3): verify_lockout cannot pass vacuously, and the LLM run's
// summary counts a lockout only for the agent it bought and the seller it
// bought from.
import { test } from "node:test";
import assert from "node:assert/strict";
import { connect } from "../src/client.js";
import { runBuyer, summarize, systemPrompt, type McpLike, type ModelClient, type ModelReply, type ToolCall } from "../src/llm.js";
import { RUNNER_PK, valid, writer } from "./helpers.js";

const call = (id: string, name: string, args: Record<string, unknown>): ToolCall => ({ id, name, arguments: args });

function scripted(turns: ToolCall[][]): ModelClient {
  let i = 0;
  return {
    provider: "mock",
    model: "mock-1",
    async complete(): Promise<ModelReply> {
      const tool_calls = turns[i++] ?? [];
      return { content: tool_calls.length ? `step ${i}` : "done", tool_calls, model: "mock-1", response_id: null, headers: {} };
    },
  };
}

async function buyThen(lockArgs: Record<string, unknown>) {
  const s = await connect({ AGENT_EXIT_ALLOW_WRITE: "1", AGENT_EXIT_ACCOUNT_ID: "summarizer.testnet", AGENT_EXIT_SESSION_CAP: "1000", AGENT_EXIT_RUNNER_PK: RUNNER_PK });
  const mcp: McpLike = {
    async tools() {
      const r = await s.client.listTools();
      return r.tools.map((t) => ({ name: t.name, description: t.description ?? "", inputSchema: t.inputSchema as Record<string, unknown> }));
    },
    call: (n, a) => s.call(n, a),
  };
  try {
    const t = await runBuyer({
      model: scripted([
        [call("1", "scan_agent", { agent: "cheat.testnet" })],
        [call("2", "scan_agent", { agent: "translator.testnet" })],
        [call("3", "buy_agent", { agent: "translator.testnet", max_price: 1000, confirm: true })],
        [call("4", "verify_lockout", lockArgs)],
      ]),
      mcp,
      system: systemPrompt("skill"),
      goal: "buy",
    });
    return summarize(t) as { seller_locked_out: boolean; pass: boolean };
  } finally {
    await s.close();
  }
}

test("M11 a key-less account with no probes is inconclusive, not locked out", async () => {
  const s = await writer();
  try {
    const r = await s.call("verify_lockout", { agent: "translator.testnet", account: "nobody.testnet", account_keys: [] });
    assert.ok(valid("verify_lockout", r));
    assert.equal(r.body.locked_out, false, JSON.stringify(r.body).slice(0, 400));
    assert.equal(r.body.inconclusive, true);
  } finally {
    await s.close();
  }
});

test("M11 the recorded seller is still proven locked out", async () => {
  const sum = await buyThen({ agent: "translator.testnet", account: "alice.testnet" });
  assert.equal(sum.seller_locked_out, true);
  assert.equal(sum.pass, true);
});

test("M11 a lockout of some other account does not count", async () => {
  const sum = await buyThen({ agent: "translator.testnet", account: "nobody.testnet", account_keys: [] });
  assert.equal(sum.seller_locked_out, false);
  assert.equal(sum.pass, false);
});

test("M11 a lockout on another agent does not count", async () => {
  const sum = await buyThen({ agent: "research.testnet", account: "alice.testnet" });
  assert.equal(sum.seller_locked_out, false);
  assert.equal(sum.pass, false);
});
