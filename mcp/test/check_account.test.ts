// CA1-CA6: check_account answers "who can take this account?" for any NEAR
// account, not only Agent Exit agents.
import { test } from "node:test";
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { connect } from "../src/client.js";
import { TestnetBackend } from "../src/backend.js";
import { loadConfig } from "../src/config.js";
import { loadSchemas, Tools, UPGRADE_WORDS } from "../src/tools.js";
import { REPO, valid } from "./helpers.js";

async function check(account: string) {
  const s = await connect({});
  try {
    const r = await s.call("check_account", { account });
    assert.ok(valid("check_account", r), JSON.stringify(r.body));
    return r.body as any;
  } finally {
    await s.close();
  }
}

test("CA1 an Agent Exit agent with only its runner's function-call key is locked", async () => {
  const b = await check("translator.testnet");
  assert.equal(b.verdict, "locked");
  assert.equal(b.full_access_keys, 0);
  assert.equal(b.code.published_agent_exit_build, true);
  assert.match(b.keys[0].can, /^call heartbeat, accept_job, pay, pay_base on translator\.testnet/);
});

test("CA2 the cheat's hidden full-access key is a backdoor risk", async () => {
  const b = await check("cheat.testnet");
  assert.equal(b.verdict, "backdoor_risk");
  assert.ok(b.findings.some((f: any) => f.code === "full_access_key_on_contract" && f.severity === "high"));
});

test("CA3 any account: a wallet is key-controlled; a bot with an upgrade method and an open function-call key is flagged; a keyless vault is locked", async () => {
  assert.equal((await check("ordinary-wallet.testnet")).verdict, "key_controlled");
  const bot = await check("trading-bot.testnet");
  assert.equal(bot.verdict, "backdoor_risk");
  const codes = bot.findings.map((f: any) => f.code).sort();
  assert.deepEqual(codes, ["code_change_method", "fc_key_any_method", "fc_key_no_allowance", "full_access_key_on_contract"]);
  const vault = await check("locked-vault.testnet");
  assert.equal(vault.verdict, "locked");
  assert.equal(vault.code.published_agent_exit_build, false);
});

test("CA4 an unknown account is an error, nothing is sent", async () => {
  const s = await connect({});
  try {
    const r = await s.call("check_account", { account: "nobody-here.testnet" });
    assert.equal(r.ok, false);
    assert.equal((r.body as any).error.code, "UNKNOWN_ACCOUNT");
  } finally {
    await s.close();
  }
});

test("CA5 testnet: any .testnet account is read from two providers, and a disagreement is a backdoor risk", async () => {
  const keysA = [{ public_key: "ed25519:A", access_key: { permission: "FullAccess" } }];
  const f = (async (url: string, init?: RequestInit) => {
    const p = JSON.parse(String(init?.body)).params;
    let result: unknown;
    if (p.request_type === "view_account") result = { code_hash: "11111111111111111111111111111111" };
    else if (p.request_type === "view_access_key_list") result = { keys: url.includes("fastnear") ? [] : keysA };
    else result = {};
    return new Response(JSON.stringify({ result }), { status: 200 });
  }) as unknown as typeof fetch;
  const cfg = loadConfig({ AGENT_EXIT_NETWORK: "testnet" });
  const t = new Tools(cfg, new TestnetBackend(cfg, f), loadSchemas(REPO));
  const r = await t.call("check_account", { account: "someone-else.testnet" });
  assert.ok(valid("check_account", r), JSON.stringify(r.body));
  const body: any = r.body;
  assert.equal(body.verdict, "key_controlled");
  assert.ok(body.findings.some((x: any) => x.code === "providers_disagree"));
});

test("CA6 the code-change words are the scanner's (tools/scan UPGRADE_WORDS)", () => {
  const src = readFileSync(`${REPO}/tools/scan/src/lib.rs`, "utf8");
  const block = src.slice(src.indexOf("pub const UPGRADE_WORDS"), src.indexOf("];", src.indexOf("pub const UPGRADE_WORDS")));
  assert.deepEqual([...block.matchAll(/"([a-z_]+)"/g)].map((m) => m[1]), UPGRADE_WORDS);
});
