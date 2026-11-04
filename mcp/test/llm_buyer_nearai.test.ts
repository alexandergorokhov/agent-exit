// NEAR AI follow-ups from the first real run. Scenario ids: LB8 model chosen
// at startup, LB9 token usage, LB10 HTTP 401/402, LB11 repeated key,
// LB12 llm-buyer-check.
import { test } from "node:test";
import assert from "node:assert/strict";
import { spawnSync } from "node:child_process";
import { join } from "node:path";
import { connect, REPO } from "../src/client.js";
import {
  AnthropicClient,
  chooseModel,
  ModelApiError,
  NEARAI_DEFAULT_MODEL,
  OpenAICompatClient,
  repeatedKey,
  runBuyer,
  selectModel,
  systemPrompt,
  totalUsage,
  type McpLike,
  type Message,
  type ModelClient,
} from "../src/llm.js";
import { RUNNER_PK } from "./helpers.js";
// A path that never exists, even where /nonexistent does (review 2 L7).
const NO_ENV_FILE = `${process.env.TMPDIR ?? "/tmp"}/agent-exit-no-env-${process.pid}-${Date.now()}/agent-exit.env`;

const SECRET = "nearai-key-0123456789abcdef";
const toolDefs = [{ name: "scan_agent", description: "scan", inputSchema: { type: "object" } }];
const msgs: Message[] = [{ role: "user", content: "buy" }];
const DEFAULT = "deepseek-ai/DeepSeek-V4.1-Flash-1017";
const req = { system: "S", messages: msgs, tools: toolDefs };

type Seen = { url?: string; init?: RequestInit };
const jsonFetch = (status: number, body: unknown, seen: Seen[] = []) =>
  (async (u: string, i?: RequestInit) => {
    seen.push({ url: u, init: i });
    return new Response(typeof body === "string" ? body : JSON.stringify(body), { status });
  }) as unknown as typeof fetch;
const P = (n: number) => ({ amount: n, scale: 9, currency: "USD" });

async function server() {
  const s = await connect({
    AGENT_EXIT_ALLOW_WRITE: "1",
    AGENT_EXIT_ACCOUNT_ID: "summarizer.testnet",
    AGENT_EXIT_SESSION_CAP: "1000",
    AGENT_EXIT_RUNNER_PK: RUNNER_PK,
  });
  const mcp: McpLike = {
    async tools() {
      const r = await s.client.listTools();
      return r.tools.map((t) => ({ name: t.name, description: t.description ?? "", inputSchema: t.inputSchema as Record<string, unknown> }));
    },
    call: (n, a) => s.call(n, a),
  };
  return { s, mcp };
}
const run = (model: ModelClient, mcp: McpLike) => runBuyer({ model, mcp, system: systemPrompt("skill"), goal: "buy" });

test("LB8 the model is the cheapest chat model that supports tools; else the default; always logged", async () => {
  const list = {
    data: [
      { id: "big/Model-A", inputCostPerToken: P(900), outputCostPerToken: P(900), supported_parameters: ["tools"] },
      { id: "cheap/no-tools", inputCostPerToken: P(1), outputCostPerToken: P(1), supported_parameters: ["temperature"] },
      { id: "cheap/embed-1", inputCostPerToken: P(1), outputCostPerToken: P(1), supported_parameters: ["tools"] },
      { id: "mid/Model-B", pricing: { prompt: "0.0000002", completion: "0.0000004" }, capabilities: { tools: true } },
      { id: "mid/Model-C", pricing: { prompt: 0.0000002, completion: 0.0000004 }, supports_tools: true },
      { id: "free/unpriced", supported_parameters: ["tools"] },
    ],
  };
  assert.equal(chooseModel(list)?.id, "mid/Model-B"); // a tie goes to the smaller id
  assert.equal(chooseModel({ data: [] }), null);
  assert.equal(chooseModel("junk"), null);
  const logs: string[] = [];
  const seen: Seen[] = [];
  const c = new OpenAICompatClient(SECRET, DEFAULT, undefined, jsonFetch(200, list, seen));
  await selectModel(c, {}, (l) => logs.push(l));
  assert.equal(c.model, "mid/Model-B");
  assert.equal(seen[0].url, "https://cloud-api.near.ai/v1/models");
  assert.equal((seen[0].init?.headers as Record<string, string> | undefined)?.authorization, undefined); // public list: no key sent
  assert.match(logs.join("\n"), /mid\/Model-B.*cheapest with tool support/);
  // NEARAI_MODEL wins and the list is not read
  const seen2: Seen[] = [];
  const c2 = new OpenAICompatClient(SECRET, "pinned/m", undefined, jsonFetch(200, list, seen2));
  await selectModel(c2, { NEARAI_MODEL: "pinned/m" }, (l) => logs.push(l));
  assert.equal(c2.model, "pinned/m");
  assert.equal(seen2.length, 0);
  // list failure or no candidate: the default stays, and the log says so
  for (const f of [jsonFetch(500, "x"), jsonFetch(200, { data: [{ id: "a/b" }] })]) {
    const l: string[] = [];
    const c3 = new OpenAICompatClient(SECRET, DEFAULT, undefined, f);
    await selectModel(c3, {}, (x) => l.push(x));
    assert.equal(c3.model, DEFAULT);
    assert.match(l[0], /using deepseek-ai\/DeepSeek-V4\.1-Flash-1017/);
  }
  assert.equal(NEARAI_DEFAULT_MODEL, DEFAULT);
});

test("LB9 token usage is recorded per reply and in total", async () => {
  const f = jsonFetch(200, { id: "c1", model: "m", usage: { prompt_tokens: 120, completion_tokens: 30 }, choices: [{ message: { content: "hi" } }] });
  const r = await new OpenAICompatClient(SECRET, "m", undefined, f).complete(req);
  assert.deepEqual(r.usage, { prompt_tokens: 120, completion_tokens: 30 });
  const a = new AnthropicClient(SECRET, "m", jsonFetch(200, { id: "x", model: "m", usage: { input_tokens: 7, output_tokens: 3 }, content: [{ type: "text", text: "t" }] }));
  assert.deepEqual((await a.complete(req)).usage, { prompt_tokens: 7, completion_tokens: 3 });
  const none = new OpenAICompatClient(SECRET, "m", undefined, jsonFetch(200, { choices: [{ message: { content: "hi" } }] }));
  assert.equal((await none.complete(req)).usage, undefined);
  // through the loop: one usage per step, summed
  const { mcp, s } = await server();
  let n = 0;
  const model: ModelClient = {
    provider: "mock",
    model: "m",
    async complete() {
      n++;
      return {
        content: "",
        tool_calls: n === 1 ? [{ id: "1", name: "list_agents", arguments: {} }] : [],
        model: "m",
        response_id: null,
        headers: {},
        usage: n === 1 ? { prompt_tokens: 10, completion_tokens: 2 } : { prompt_tokens: 20, completion_tokens: 5 },
      };
    },
  };
  const t = await run(model, mcp);
  await s.close();
  assert.deepEqual(t.steps.map((x) => x.usage), [{ prompt_tokens: 10, completion_tokens: 2 }, { prompt_tokens: 20, completion_tokens: 5 }]);
  assert.deepEqual(totalUsage(t.steps), { replies_with_usage: 2, prompt_tokens: 30, completion_tokens: 7, total_tokens: 37 });
});

test("LB10 HTTP 401 and 402 end the run with a clear message and are not retried", async () => {
  for (const [status, re] of [[401, /key rejected/], [402, /no credits\/spending limit/]] as const) {
    let calls = 0;
    const f = (async () => {
      calls++;
      return new Response(`nope ${SECRET}`, { status });
    }) as unknown as typeof fetch;
    const c = new OpenAICompatClient(SECRET, "m", undefined, f);
    await assert.rejects(c.complete(req), (e: Error) => {
      assert.match(e.message, re);
      assert.ok(!e.message.includes(SECRET));
      assert.equal((e as ModelApiError).status, status);
      return true;
    });
    const { mcp, s } = await server();
    const t = await run(c, mcp);
    await s.close();
    assert.equal(calls, 2); // one direct call above, one in the loop: no retry
    assert.equal(t.status, "model_error");
    assert.equal(t.error_status, status);
    assert.match(t.error ?? "", re);
  }
  const msg = await new AnthropicClient(SECRET, "m", jsonFetch(401, "x")).complete(req).then(
    () => "",
    (e: Error) => e.message,
  );
  assert.match(msg, /Anthropic: key rejected/);
});

test("LB11 a key pasted several times is detected without printing it", () => {
  assert.equal(repeatedKey("abcd1234efgh".repeat(3)), 3);
  assert.equal(repeatedKey("sk-abcdefgh".repeat(2)), 2);
  assert.equal(repeatedKey("sk-abcdefghijklmnop"), null);
  assert.equal(repeatedKey(""), null);
  const key = "sk-live-0123456789abcdef";
  const r = spawnSync(process.execPath, [join(REPO, "mcp", "dist", "scripts", "llm-buyer.js"), "--check"], {
    env: { PATH: process.env.PATH, AGENT_EXIT_ENV: NO_ENV_FILE, NEARAI_API_KEY: key.repeat(3), NEARAI_BASE_URL: "https://127.0.0.1:1/v1" },
    encoding: "utf8",
  });
  assert.match(r.stderr, /NEARAI_API_KEY is one string of 24 characters repeated 3 times/);
  assert.ok(!r.stderr.includes(key) && !r.stdout.includes(key));
});

test("LB12 llm-buyer --check makes one tiny authenticated chat call; 401/402 get clear errors", async () => {
  const seen: Seen[] = [];
  const ok = { id: "c", model: "m", usage: { prompt_tokens: 9, completion_tokens: 1 }, choices: [{ message: { content: "OK" } }] };
  const r = await new OpenAICompatClient(SECRET, "m", undefined, jsonFetch(200, ok, seen)).ping!();
  assert.equal(seen.length, 1);
  assert.equal(seen[0].url, "https://cloud-api.near.ai/v1/chat/completions");
  assert.equal((seen[0].init?.headers as Record<string, string>).authorization, `Bearer ${SECRET}`);
  const body = JSON.parse(seen[0].init?.body as string);
  assert.ok(body.max_tokens <= 8 && !("tools" in body));
  assert.deepEqual(r.usage, { prompt_tokens: 9, completion_tokens: 1 });
  await assert.rejects(new OpenAICompatClient(SECRET, "m", undefined, jsonFetch(402, "x")).ping!(), /no credits\/spending limit/);
  await assert.rejects(new OpenAICompatClient(SECRET, "m", undefined, jsonFetch(401, "x")).ping!(), /key rejected/);
  // the script with no key: exit 2, nothing started
  const none = spawnSync(process.execPath, [join(REPO, "mcp", "dist", "scripts", "llm-buyer.js"), "--check"], {
    env: { PATH: process.env.PATH, AGENT_EXIT_ENV: NO_ENV_FILE },
    encoding: "utf8",
  });
  assert.equal(none.status, 2);
  assert.match(none.stderr, /needs NEARAI_API_KEY/);
});
