// M14 (review 5, L2): the LLM loop's budget holds even when the provider
// reports no token usage, and one reply cannot fan out into unlimited calls.
import { test } from "node:test";
import assert from "node:assert/strict";
import { runBuyer, type McpLike, type ModelClient, type ModelReply } from "../src/llm.js";

const big = "x".repeat(60_000);

test("M14 a provider that reports no usage still hits the token budget (estimated)", async () => {
  let calls = 0;
  const mcp: McpLike = { tools: async () => [{ name: "list_agents", description: "", inputSchema: {} }], call: async () => ({ ok: true, body: { filler: big } }) };
  const silent: ModelClient = {
    provider: "mock",
    model: "m",
    async complete(): Promise<ModelReply> {
      calls++;
      return { content: big, tool_calls: [{ id: String(calls), name: "list_agents", arguments: {} }], model: "m", response_id: null, headers: {} };
    },
  };
  const t = await runBuyer({ model: silent, mcp, system: "s", goal: "g", maxSteps: 50, maxTotalTokens: 100_000 });
  assert.equal(t.status, "budget");
  assert.ok(calls < 10, `made ${calls} model calls with no usage reported`);
});

test("M14 at most 8 tool calls per reply are forwarded; the rest are refused", async () => {
  const forwarded: string[] = [];
  const mcp: McpLike = {
    tools: async () => [{ name: "list_agents", description: "", inputSchema: {} }],
    call: async (n) => {
      forwarded.push(n);
      return { ok: true, body: {} };
    },
  };
  let i = 0;
  const fan: ModelClient = {
    provider: "mock",
    model: "m",
    async complete(): Promise<ModelReply> {
      i++;
      const tool_calls = i === 1 ? Array.from({ length: 50 }, (_, k) => ({ id: `c${k}`, name: "list_agents", arguments: {} })) : [];
      return { content: "", tool_calls, model: "m", response_id: null, headers: {}, usage: { prompt_tokens: 10, completion_tokens: 10 } };
    },
  };
  const t = await runBuyer({ model: fan, mcp, system: "s", goal: "g" });
  assert.equal(forwarded.length, 8);
  assert.equal(t.steps[0].tool_calls.length, 50, "every call id still gets an answer");
  assert.equal(t.steps[0].tool_calls.filter((c) => !c.forwarded).length, 42);
});
