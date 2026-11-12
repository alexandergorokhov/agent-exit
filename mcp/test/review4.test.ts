// Security review 4, MCP part. Scenario ids: M4-L4a (testnet buy_agent only
// for AGENT_EXIT_AGENTS), M4-L4b (the session cap survives a restart),
// M4-L5 (the model env file must be mode 600).
import { test } from "node:test";
import assert from "node:assert/strict";
import { chmodSync, existsSync, mkdtempSync, readFileSync, statSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { spawnSync } from "node:child_process";
import { OfflineBackend } from "../src/backend.js";
import { loadConfig } from "../src/config.js";
import { checkSecretFileMode, loadModelEnv } from "../src/llm.js";
import { loadSchemas, Tools } from "../src/tools.js";
import { REPO, RUNNER_PK } from "./helpers.js";

const WORLD = join(REPO, "fixtures", "mcp", "world.json");
const baseEnv = { AGENT_EXIT_ALLOW_WRITE: "1", AGENT_EXIT_ACCOUNT_ID: "summarizer.testnet", AGENT_EXIT_RUNNER_PK: RUNNER_PK };
const mk = (env: Record<string, string>, backend = new OfflineBackend(WORLD)) => new Tools(loadConfig({ ...baseEnv, ...env }), backend, loadSchemas(REPO));
const BUY = { agent: "translator.factory.testnet", max_price: 1000, confirm: true };
const tmp = () => mkdtempSync(join(tmpdir(), "m4-"));

/** An offline world that counts every read, so a refusal can be shown to happen first. */
class Counting extends OfflineBackend {
  reads = 0;
  override async agent(id: string) {
    this.reads++;
    return super.agent(id);
  }
}

test("M4-L4a on testnet, buy_agent only buys agents in AGENT_EXIT_AGENTS, before any read", async () => {
  const env = { AGENT_EXIT_NETWORK: "testnet", AGENT_EXIT_PRIVATE_KEY: "ed25519:x" };
  // not in the list
  let b = new Counting(WORLD);
  let r = await mk({ ...env, AGENT_EXIT_AGENTS: "other.testnet" }, b).call("buy_agent", BUY);
  assert.equal(r.ok, false);
  assert.equal(r.body.error.code, "UNKNOWN_AGENT");
  assert.match(r.body.error.message, /AGENT_EXIT_AGENTS/);
  assert.equal(r.body.sent, false);
  assert.equal(b.reads, 0);
  // an empty list allows nothing
  b = new Counting(WORLD);
  r = await mk(env, b).call("buy_agent", BUY);
  assert.equal(r.body.error.code, "UNKNOWN_AGENT");
  assert.equal(b.reads, 0);
  // in the list: gets past the check
  b = new Counting(WORLD);
  await mk({ ...env, AGENT_EXIT_AGENTS: "translator.factory.testnet", AGENT_EXIT_SPEND_FILE: join(tmp(), "s.json") }, b).call("buy_agent", BUY);
  assert.ok(b.reads > 0);
  // offline keeps its fixtures: no list needed
  r = await mk({}).call("buy_agent", BUY);
  assert.equal(r.ok, true, JSON.stringify(r.body));
});

test("M4-L4b the spend cap persists per account and day, and restarts do not reset it", async () => {
  const file = join(tmp(), "spend.json");
  const env = { AGENT_EXIT_SESSION_CAP: "1500", AGENT_EXIT_SPEND_FILE: file };
  const first = await mk(env).call("buy_agent", BUY);
  assert.equal(first.ok, true, JSON.stringify(first.body));
  assert.equal(statSync(file).mode & 0o777, 0o600);
  const day = new Date().toISOString().slice(0, 10);
  assert.deepEqual(JSON.parse(readFileSync(file, "utf8")), { "summarizer.testnet": { [day]: "1000000000" } });
  // a new server (a restart) starts with 1000 already spent: a second 1000 is over 1500
  const second = await mk(env).call("buy_agent", BUY);
  assert.equal(second.ok, false);
  assert.equal(second.body.error.code, "SESSION_CAP");
  assert.equal(second.body.sent, false);
  // the refusal released nothing it did not take
  assert.equal(JSON.parse(readFileSync(file, "utf8"))["summarizer.testnet"][day], "1000000000");
  // another account has its own budget
  writeFileSync(file, JSON.stringify({ "other-buyer.testnet": { [day]: "9999999999999" } }), { mode: 0o600 });
  const other = await mk(env).call("buy_agent", BUY);
  assert.equal(other.ok, true, JSON.stringify(other.body));
  // yesterday's spend does not count today
  writeFileSync(file, JSON.stringify({ "summarizer.testnet": { "2000-01-01": "9999999999999" } }), { mode: 0o600 });
  assert.equal((await mk(env).call("buy_agent", BUY)).ok, true);
  // a file that cannot be read is a refusal, not a free pass
  writeFileSync(file, "{not json", { mode: 0o600 });
  const bad = await mk(env).call("buy_agent", BUY);
  assert.equal(bad.ok, false);
  assert.equal(bad.body.error.code, "SESSION_CAP");
  assert.match(bad.body.error.message, /spend file/);
});

test("M4-L4b the default spend file: testnet persists in out/, offline stays in memory", () => {
  assert.match(loadConfig({ AGENT_EXIT_NETWORK: "testnet" }).spendFile ?? "", /out[\\/]mcp-spend\.json$/);
  assert.equal(loadConfig({}).spendFile, null);
  assert.equal(loadConfig({ AGENT_EXIT_SPEND_FILE: "/x/y.json" }).spendFile, "/x/y.json");
});

test("M4-L5 the model env file is refused unless it is mode 600", () => {
  const f = join(tmp(), "env");
  writeFileSync(f, "NEARAI_API_KEY=abcdefghijkl\n");
  for (const mode of [0o644, 0o640, 0o604, 0o666]) {
    chmodSync(f, mode);
    assert.throws(() => loadModelEnv({ AGENT_EXIT_ENV: f }), /chmod 600/, mode.toString(8));
  }
  chmodSync(f, 0o600);
  assert.equal(loadModelEnv({ AGENT_EXIT_ENV: f }).NEARAI_API_KEY, "abcdefghijkl");
  assert.doesNotThrow(() => checkSecretFileMode(join(tmp(), "missing")));
  chmodSync(f, 0o644);
  const r = spawnSync(process.execPath, [join(REPO, "mcp", "dist", "scripts", "llm-buyer.js"), "--check"], {
    env: { PATH: process.env.PATH, AGENT_EXIT_ENV: f },
    encoding: "utf8",
  });
  assert.equal(r.status, 2);
  assert.match(r.stderr, /chmod 600/);
  assert.ok(!r.stderr.includes("abcdefghijkl") && !existsSync("/nonexistent-never"));
});
