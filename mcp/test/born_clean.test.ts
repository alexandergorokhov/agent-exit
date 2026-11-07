// BC16-BC18: the MCP server hands the scan the born-clean factory's record
// (contracts/factory) when AGENT_EXIT_FACTORY is set. BC16 a factory-born
// agent's record is read and passed on, BC17 an unreadable record becomes one
// that matches nothing (the scan blocks), BC18 without a factory there is no
// born check and scan_agent reports born_clean null.
import { test } from "node:test";
import assert from "node:assert/strict";
import { TestnetBackend } from "../src/backend.js";
import { loadConfig } from "../src/config.js";
import { valid } from "./helpers.js";
import { connect } from "../src/client.js";

const FACTORY = "factory.agentexit4.testnet";
const AGENT = `born1.${FACTORY}`;
const HASH = "11111111111111111111111111111111";

function fakeChain(o: { record?: unknown; failRecord?: boolean } = {}) {
  const asked: string[] = [];
  const f = (async (_url: string, init?: RequestInit) => {
    const p = JSON.parse(String(init?.body)).params;
    const view = (v: unknown) => ({ result: [...Buffer.from(JSON.stringify(v))] });
    let result: unknown;
    if (p.request_type === "call_function") {
      asked.push(`${p.account_id}.${p.method_name}`);
      if (p.account_id === FACTORY && p.method_name === "get_agent") {
        if (o.failRecord) return new Response(JSON.stringify({ result: { error: "MethodNotFound" } }), { status: 200 });
        result = view(o.record === undefined ? { agent: AGENT, code_hash: HASH, state: "Born", owner_at_birth: "alice.testnet" } : o.record);
      } else if (p.method_name === "get_status") {
        result = view({ status: "Active", owner: "alice.testnet", listing_version: 0, known_keys: [], known_keys_hash: "00", open_approvals: 0, frozen: false, admin: "admin.testnet", token: "xusd.testnet", paid_to_total: 0, near_storage: { solvent: true } });
      } else if (p.method_name === "get_listing") result = view(null);
      else return new Response(JSON.stringify({ result: { error: `unexpected ${p.method_name}` } }), { status: 200 });
    } else if (p.request_type === "view_access_key_list") result = { keys: [] };
    else if (p.request_type === "view_account") result = { code_hash: HASH };
    else result = { code_base64: Buffer.from([0, 0x61, 0x73, 0x6d, 1, 0, 0, 0]).toString("base64") };
    return new Response(JSON.stringify({ result }), { status: 200 });
  }) as unknown as typeof fetch;
  return { f, asked };
}

const cfg = (factory?: string) => loadConfig({ AGENT_EXIT_NETWORK: "testnet", ...(factory ? { AGENT_EXIT_FACTORY: factory } : {}) });

test("BC16 a factory-born agent's birth record goes into the scan input", async () => {
  const c = fakeChain();
  const input = (await new TestnetBackend(cfg(FACTORY), c.f).scanInput(AGENT, {})) as { born?: { factory: string; record: Record<string, string> | null } };
  assert.equal(input.born?.factory, FACTORY);
  assert.deepEqual(input.born?.record, { agent: AGENT, code_hash: HASH, state: "Born", owner_at_birth: "alice.testnet" });
  assert.ok(c.asked.includes(`${FACTORY}.get_agent`));
});

test("BC16b an agent that is not a child of the factory has no record (and the factory is not asked)", async () => {
  const c = fakeChain();
  const input = (await new TestnetBackend(cfg(FACTORY), c.f).scanInput("translator.agentexit4.testnet", {})) as { born?: { record: unknown } };
  assert.equal(input.born?.record, null);
  assert.ok(!c.asked.includes(`${FACTORY}.get_agent`));
});

test("BC17 an unreadable birth record matches nothing, so the scan cannot call the agent born clean", async () => {
  const input = (await new TestnetBackend(cfg(FACTORY), fakeChain({ failRecord: true }).f).scanInput(AGENT, {})) as { born?: { record: Record<string, string> } };
  assert.equal(input.born?.record.code_hash, "unreadable");
  assert.equal(input.born?.record.state, "unreadable");
});

test("BC18 without AGENT_EXIT_FACTORY there is no born check; offline scan_agent reports born_clean null", async () => {
  const input = (await new TestnetBackend(cfg(), fakeChain().f).scanInput(AGENT, {})) as { born?: unknown };
  assert.equal(input.born, undefined);
  const s = await connect({});
  try {
    const r = await s.call("scan_agent", { agent: "translator.testnet" });
    assert.ok(valid("scan_agent", r));
    const body: any = r.body;
    assert.equal(body.born_clean, null);
  } finally {
    await s.close();
  }
});
