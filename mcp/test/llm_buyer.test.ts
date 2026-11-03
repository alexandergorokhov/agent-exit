// The LLM buyer loop, tested with a mocked model client against the real MCP
// server (offline). Scenario ids: LB1 happy path, LB2 the server (not the
// model) enforces max_price/confirm/cap, LB3 unknown tools and bad arguments,
// LB4 provider requests and secrets, LB5 no key -> exit 2, LB6 env file and
// base URL guards, LB7 step limit and model errors.
import { test } from "node:test";
import assert from "node:assert/strict";
import { spawnSync } from "node:child_process";
import { join } from "node:path";
import { connect, REPO } from "../src/client.js";
import {
  AnthropicClient,
  checkBaseUrl,
  keyFromPointer,
  loadModelEnv,
  OpenAICompatClient,
  parseEnvFile,
  pickClient,
  runBuyer,
  summarize,
  systemPrompt,
  type McpLike,
  type Message,
  type ModelClient,
  type ModelReply,
  type ToolCall,
} from "../src/llm.js";
import { RUNNER_PK } from "./helpers.js";

async function server(cap = "1000") {
  const s = await connect({
    AGENT_EXIT_ALLOW_WRITE: "1",
    AGENT_EXIT_ACCOUNT_ID: "summarizer.testnet",
    AGENT_EXIT_SESSION_CAP: cap,
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

const call = (id: string, name: string, args: Record<string, unknown> | null): ToolCall => ({ id, name, arguments: args });
/** A model that plays back one list of tool calls per turn, then stops. */
function scripted(turns: ToolCall[][], seen: Message[][] = []): ModelClient {
  let i = 0;
  return {
    provider: "mock",
    model: "mock-1",
    async complete(req): Promise<ModelReply> {
      seen.push(structuredClone(req.messages));
      const tool_calls = turns[i++] ?? [];
      return { content: tool_calls.length ? `step ${i}` : "done", tool_calls, model: "mock-1", response_id: `chat-${i}`, headers: { "x-signature": "abc" } };
    },
  };
}
const run = (model: ModelClient, mcp: McpLike, maxSteps?: number) =>
  runBuyer({ model, mcp, system: systemPrompt("skill text"), goal: "buy", maxSteps });

test("LB1 a model scans, refuses cheat, buys translator and verifies the lockout", async () => {
  const { s, mcp } = await server();
  try {
    const seen: Message[][] = [];
    const model = scripted(
      [
        [call("1", "list_agents", { status: "Listed" })],
        [call("2", "scan_agent", { agent: "cheat.testnet" }), call("3", "scan_agent", { agent: "translator.testnet" })],
        [call("4", "buy_agent", { agent: "cheat.testnet", max_price: 600, confirm: true })],
        [call("5", "buy_agent", { agent: "translator.testnet", max_price: 1000, confirm: true })],
        [call("6", "verify_lockout", { agent: "translator.testnet", account: "alice.testnet" })],
      ],
      seen,
    );
    const t = await run(model, mcp);
    assert.equal(t.status, "done");
    const sum = summarize(t) as Record<string, unknown>;
    assert.deepEqual(sum.scanned_cheat, ["blocked"]);
    assert.deepEqual(sum.cheat_buy_attempts, ["SCAN_BLOCKED"]);
    assert.equal(sum.cheat_bought, false);
    assert.equal(sum.translator_bought, true);
    assert.equal(sum.seller_locked_out, true);
    assert.equal(sum.pass, true);
    // Tool results went back to the model as tool messages, in order.
    const last = seen[seen.length - 1];
    assert.deepEqual(last.filter((m) => m.role === "tool").map((m) => (m as { tool_call_id: string }).tool_call_id), ["1", "2", "3", "4", "5", "6"]);
    // Headers of every reply are kept for the transcript.
    assert.equal(t.steps[0].response_headers["x-signature"], "abc");
    assert.equal(t.steps[0].model_id, "mock-1");
  } finally {
    await s.close();
  }
});

test("LB2 the server, not the model, enforces confirm, max_price and the cap", async () => {
  const { s, mcp } = await server("1000");
  try {
    const t = await run(
      scripted([
        [
          call("a", "buy_agent", { agent: "translator.testnet", max_price: 1000 }), // no confirm
          call("b", "buy_agent", { agent: "translator.testnet", max_price: 10, confirm: true }), // under price
          call("c", "buy_agent", { agent: "cheat.testnet", max_price: 1_000_000, confirm: true }), // any max_price
          call("d", "buy_agent", { agent: "translator.testnet", max_price: 1000, confirm: true, buyer_runner_pk: "ed25519:evil" }), // extra arg
        ],
      ]),
      mcp,
    );
    const codes = t.steps[0].tool_calls.map((c) => (c.result as { error?: { code: string } }).error?.code);
    assert.deepEqual(codes, ["BAD_INPUT", "OVER_MAX_PRICE", "SCAN_BLOCKED", "BAD_INPUT"]);
    // The loop passed the model's arguments through untouched.
    assert.deepEqual(t.steps[0].tool_calls[0].arguments, { agent: "translator.testnet", max_price: 1000 });
    assert.equal((summarize(t) as { translator_bought: boolean }).translator_bought, false);
    // A cap below the price refuses a well-formed buy.
    const low = await server("999");
    try {
      const t2 = await run(scripted([[call("e", "buy_agent", { agent: "translator.testnet", max_price: 1000, confirm: true })]]), low.mcp);
      assert.equal((t2.steps[0].tool_calls[0].result as { error: { code: string } }).error.code, "SESSION_CAP");
    } finally {
      await low.s.close();
    }
  } finally {
    await s.close();
  }
});

test("LB3 unknown tools and non-object arguments are never forwarded", async () => {
  const forwarded: string[] = [];
  const mcp: McpLike = {
    tools: async () => [{ name: "list_agents", description: "", inputSchema: {} }],
    call: async (n) => {
      forwarded.push(n);
      return { ok: true, body: {} };
    },
  };
  const t = await run(scripted([[call("1", "run_shell", { cmd: "cat ~/.agent-exit.env" }), call("2", "list_agents", null)]]), mcp);
  assert.deepEqual(forwarded, []);
  assert.deepEqual(t.steps[0].tool_calls.map((c) => c.forwarded), [false, false]);
  assert.match(JSON.stringify(t.steps[0].tool_calls[0].result), /no such tool/);
});

test("LB7 the step limit and model errors end the run", async () => {
  const mcp: McpLike = { tools: async () => [{ name: "list_agents", description: "", inputSchema: {} }], call: async () => ({ ok: true, body: {} }) };
  const forever = scripted(Array.from({ length: 50 }, (_, i) => [call(String(i), "list_agents", {})]));
  const t = await run(forever, mcp, 3);
  assert.equal(t.status, "max_steps");
  assert.equal(t.steps.length, 3);
  const boom: ModelClient = { provider: "mock", model: "m", complete: async () => Promise.reject(new Error("HTTP 500")) };
  const e = await run(boom, mcp);
  assert.equal(e.status, "model_error");
  assert.equal((summarize(e) as { pass: boolean }).pass, false);
});

test("LB13 a token budget ends the run before the next model call (owner's API credit is capped)", async () => {
  const mcp: McpLike = { tools: async () => [{ name: "list_agents", description: "", inputSchema: {} }], call: async () => ({ ok: true, body: {} }) };
  let calls = 0;
  const heavy: ModelClient = {
    provider: "mock",
    model: "m",
    async complete(): Promise<ModelReply> {
      calls++;
      return { content: "", tool_calls: [call(String(calls), "list_agents", {})], model: "m", response_id: null, headers: {}, usage: { prompt_tokens: 30_000, completion_tokens: 1_000 } };
    },
  };
  const t = await runBuyer({ model: heavy, mcp, system: "s", goal: "g", maxSteps: 50, maxTotalTokens: 100_000 });
  assert.equal(t.status, "budget");
  assert.equal(calls, 4, "stops once 4 x 31k > 100k, never makes a 5th call");
  assert.equal((summarize(t) as { pass: boolean }).pass, false);
});

// ------------------------------------------------------------------ providers

const SECRET = "nearai-key-0123456789abcdef";
const toolDefs = [{ name: "scan_agent", description: "scan", inputSchema: { type: "object" } }];
const msgs: Message[] = [
  { role: "user", content: "goal" },
  { role: "assistant", content: "", tool_calls: [call("t1", "scan_agent", { agent: "x.testnet" })] },
  { role: "tool", tool_call_id: "t1", name: "scan_agent", content: "{}" },
];

test("LB4 NEAR AI request: OpenAI chat completions, key only in the header, headers kept", async () => {
  let url = "";
  let init: RequestInit | undefined;
  const f = (async (u: string, i?: RequestInit) => {
    url = u;
    init = i;
    return new Response(
      JSON.stringify({
        id: "chatcmpl-9",
        model: "deepseek-ai/DeepSeek-V3.1",
        choices: [{ message: { content: "ok", tool_calls: [{ id: "c1", type: "function", function: { name: "scan_agent", arguments: '{"agent":"cheat.testnet"}' } }] } }],
      }),
      { status: 200, headers: { "x-request-id": "r1", "x-signing-algo": "ecdsa", "set-cookie": "no", authorization: "no" } },
    );
  }) as unknown as typeof fetch;
  const c = new OpenAICompatClient(SECRET, "deepseek-ai/DeepSeek-V3.1", "https://cloud-api.near.ai/v1/", f);
  const r = await c.complete({ system: "SYS", messages: msgs, tools: toolDefs });
  assert.equal(url, "https://cloud-api.near.ai/v1/chat/completions");
  assert.equal((init?.headers as Record<string, string>).authorization, `Bearer ${SECRET}`);
  assert.equal(init?.redirect, "error");
  assert.ok(init?.signal);
  const body = JSON.parse(init?.body as string);
  assert.ok(!JSON.stringify(body).includes(SECRET));
  assert.deepEqual(body.messages.map((m: { role: string }) => m.role), ["system", "user", "assistant", "tool"]);
  assert.equal(body.messages[2].tool_calls[0].function.arguments, '{"agent":"x.testnet"}');
  assert.equal(body.tools[0].function.name, "scan_agent");
  assert.deepEqual(r.tool_calls[0].arguments, { agent: "cheat.testnet" });
  assert.equal(r.response_id, "chatcmpl-9");
  assert.equal(r.headers["x-request-id"], "r1");
  assert.equal(r.headers["x-signing-algo"], "ecdsa");
  assert.equal(r.headers["set-cookie"], undefined);
  assert.equal(r.headers.authorization, undefined);
});

test("LB4 an API error never repeats the key", async () => {
  const f = (async () => new Response(`bad key ${SECRET}`, { status: 401 })) as unknown as typeof fetch;
  const c = new OpenAICompatClient(SECRET, "m", undefined, f);
  await assert.rejects(c.complete({ system: "s", messages: msgs, tools: toolDefs }), (e: Error) => /401/.test(e.message) && !e.message.includes(SECRET));
  const net = (async () => {
    throw new Error(`connect failed for key ${SECRET}`);
  }) as unknown as typeof fetch;
  await assert.rejects(new OpenAICompatClient(SECRET, "m", undefined, net).complete({ system: "s", messages: msgs, tools: toolDefs }), (e: Error) => !e.message.includes(SECRET));
});

test("LB4 a signature lookup is keyed by the chat id and failures are recorded", async () => {
  let url = "";
  const f = (async (u: string) => {
    url = u;
    return new Response(JSON.stringify({ text: "t", signature: "0xsig", signing_address: "0xabc", signing_algo: "ecdsa" }), { status: 200 });
  }) as unknown as typeof fetch;
  const c = new OpenAICompatClient(SECRET, "m", undefined, f);
  const sig = (await c.signature("chat/../1")) as { signature: string };
  assert.equal(sig.signature, "0xsig");
  assert.match(url, /\/v1\/signature\/chat%2F\.\.%2F1\?model=m&signing_algo=ecdsa$/);
  const down = new OpenAICompatClient(SECRET, "m", undefined, (async () => new Response("", { status: 404 })) as unknown as typeof fetch);
  assert.deepEqual(await down.signature("x"), { error: "HTTP 404" });
});

test("LB4 Anthropic fallback request and reply", async () => {
  let init: RequestInit | undefined;
  const f = (async (_u: string, i?: RequestInit) => {
    init = i;
    return new Response(
      JSON.stringify({ id: "msg_1", model: "claude-haiku-4-5", content: [{ type: "text", text: "hi" }, { type: "tool_use", id: "tu1", name: "scan_agent", input: { agent: "a.testnet" } }] }),
      { status: 200 },
    );
  }) as unknown as typeof fetch;
  const c = new AnthropicClient("sk-ant-0123456789", "claude-haiku-4-5", f);
  const two: Message[] = [...msgs, { role: "tool", tool_call_id: "t2", name: "scan_agent", content: "{}" }];
  const r = await c.complete({ system: "SYS", messages: two, tools: toolDefs });
  assert.equal((init?.headers as Record<string, string>)["x-api-key"], "sk-ant-0123456789");
  const body = JSON.parse(init?.body as string);
  assert.equal(body.system, "SYS");
  assert.equal(body.tools[0].input_schema.type, "object");
  // Two tool results in a row become one user message.
  assert.deepEqual(body.messages.map((m: { role: string }) => m.role), ["user", "assistant", "user"]);
  assert.equal(body.messages[2].content.length, 2);
  assert.deepEqual(r.tool_calls[0].arguments, { agent: "a.testnet" });
  assert.equal(r.content, "hi");
});

test("LB4 invalid JSON arguments become null, not a guess", async () => {
  const f = (async () =>
    new Response(JSON.stringify({ choices: [{ message: { content: null, tool_calls: [{ id: "x", function: { name: "buy_agent", arguments: "{max_price: 5" } }] } }] }), { status: 200 })) as unknown as typeof fetch;
  const r = await new OpenAICompatClient(SECRET, "m", undefined, f).complete({ system: "s", messages: msgs, tools: toolDefs });
  assert.equal(r.tool_calls[0].arguments, null);
});

test("LB6 env file: only model settings are read; the key goes only to near.ai", () => {
  const got = parseEnvFile(
    ["# c", "export NEARAI_API_KEY='k1'", "NEARAI_MODEL=m", "AGENT_EXIT_PRIVATE_KEY=ed25519:secret", "BASE_RELAYER_KEY=0xdead", "ANTHROPIC_API_KEY=\"a1\""].join("\n"),
  );
  assert.deepEqual(got, { NEARAI_API_KEY: "k1", NEARAI_MODEL: "m", ANTHROPIC_API_KEY: "a1" });
  for (const bad of ["http://cloud-api.near.ai/v1", "https://evil.example/v1", "https://cloud-api.near.ai.evil.com/v1", "https://u:p@cloud-api.near.ai/v1", "https://cloud-api.near.ai:8443/v1", "https://notnear.ai/v1"]) {
    assert.throws(() => checkBaseUrl(bad), /refusing|not a URL/, bad);
  }
  assert.equal(checkBaseUrl("https://cloud-api.near.ai/v1/"), "https://cloud-api.near.ai/v1");
  assert.equal(pickClient({}), null);
  assert.equal(pickClient({ NEARAI_API_KEY: "kkkkkkkkkk" })?.provider, "nearai");
  assert.equal(pickClient({ ANTHROPIC_API_KEY: "kkkkkkkkkk" })?.provider, "anthropic");
  assert.equal(pickClient({ NEARAI_API_KEY: "kkkkkkkkkk", ANTHROPIC_API_KEY: "aaaaaaaaaa" })?.provider, "nearai");
  assert.throws(() => pickClient({ NEARAI_API_KEY: "kkkkkkkkkk", NEARAI_BASE_URL: "https://evil.example/v1" }));
});

test("LB14 ANTHROPIC_API_KEY_FROM: the key is read from the owner's other env file, CR and quotes stripped", async () => {
  const { mkdtempSync, writeFileSync, chmodSync } = await import("node:fs");
  const { tmpdir } = await import("node:os");
  const d = mkdtempSync(join(tmpdir(), "lb14-"));
  const other = join(d, "radar.env");
  writeFileSync(other, 'OTHER=x\r\nCLAUDE_TOKEN="sk-test-123"\r\n');
  const mine = join(d, "agent-exit.env");
  writeFileSync(mine, `ANTHROPIC_API_KEY_FROM=${other}#CLAUDE_TOKEN\nAGENT_EXIT_ROOT_KEY=ed25519:secret\n`);
  chmodSync(mine, 0o600);
  const env = loadModelEnv({ AGENT_EXIT_ENV: mine });
  assert.equal(env.ANTHROPIC_API_KEY, "sk-test-123");
  assert.equal(pickClient(env, "anthropic")?.provider, "anthropic");
  assert.equal(keyFromPointer(`${other}#MISSING`), undefined);
  assert.equal(keyFromPointer(`${d}/nope.env#CLAUDE_TOKEN`), undefined);
  assert.equal(keyFromPointer("no-hash"), undefined);
  // ANTHROPIC_WORKSPACE_ID becomes the anthropic-workspace-id header.
  let sent: Record<string, string> = {};
  const f = (async (_u: string, init: RequestInit) => {
    sent = init.headers as Record<string, string>;
    return new Response(JSON.stringify({ id: "m", model: "claude-haiku-4-5", content: [], usage: { input_tokens: 1, output_tokens: 1 } }), { status: 200 });
  }) as unknown as typeof fetch;
  await (pickClient({ ANTHROPIC_API_KEY: "kkkkkkkkkk", ANTHROPIC_WORKSPACE_ID: "wrkspc_1" }, "anthropic", f) as AnthropicClient).ping();
  assert.equal(sent["anthropic-workspace-id"], "wrkspc_1");
  await (pickClient({ ANTHROPIC_API_KEY: "kkkkkkkkkk" }, "anthropic", f) as AnthropicClient).ping();
  assert.equal(sent["anthropic-workspace-id"], undefined);
  // An explicit ANTHROPIC_API_KEY wins over the pointer.
  assert.equal(loadModelEnv({ AGENT_EXIT_ENV: mine, ANTHROPIC_API_KEY: "direct" }).ANTHROPIC_API_KEY, "direct");
});

test("LB5 no key: exit 2, 'needs NEARAI_API_KEY', nothing started", () => {
  const r = spawnSync(process.execPath, [join(REPO, "mcp", "dist", "scripts", "llm-buyer.js"), "--out", "/nonexistent-dir/never.json"], {
    env: { PATH: process.env.PATH, AGENT_EXIT_ENV: "/nonexistent" },
    encoding: "utf8",
  });
  assert.equal(r.status, 2, r.stderr);
  assert.match(r.stderr, /needs NEARAI_API_KEY/);
});
