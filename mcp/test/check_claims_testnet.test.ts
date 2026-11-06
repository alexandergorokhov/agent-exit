// J5-J6: check_claims on testnet re-reads the README's recorded transactions
// from archival nodes (mocked here: the cloud test box has no NEAR RPC).
import { test } from "node:test";
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { join } from "node:path";
import { ARCHIVAL_RPC, TestnetBackend, txOutcomeOf } from "../src/backend.js";
import { assertNotMainnet, loadConfig } from "../src/config.js";
import { Tools } from "../src/tools.js";
import { REPO, schemas } from "./helpers.js";

const data = JSON.parse(readFileSync(join(REPO, "web", "data", "testnet.json"), "utf8"));
const recorded = new Map<string, any>();
for (const st of Object.values(data.steps) as any[]) for (const t of st.txs ?? []) if (t.tx) recorded.set(t.tx, t);

/** An RPC `tx` answer shaped like the chain's, built from the recording. */
function chainAnswer(t: any, tamper = false): any {
  const logs = (t.events ?? []).map((e: string) => `EVENT_JSON:${JSON.stringify({ standard: "agent-exit", event: e })}`);
  return {
    transaction: { signer_id: tamper ? "mallory.testnet" : t.by, receiver_id: t.receiver, actions: [{ FunctionCall: { method_name: t.method } }] },
    status: t.ok ? { SuccessValue: "" } : { Failure: { ActionError: { kind: { FunctionCallError: { ExecutionError: t.error ?? "x" } } } } },
    receipts_outcome: [{ outcome: { logs, status: t.ok ? { SuccessValue: "" } : { Failure: {} } } }],
  };
}

function mockFetch(opts: { tamper?: string } = {}): { f: typeof fetch; urls: string[] } {
  const urls: string[] = [];
  const f = (async (url: string, init: any) => {
    urls.push(url);
    const body = JSON.parse(init.body);
    if (body.method === "tx") {
      const t = recorded.get(body.params.tx_hash);
      if (!t) return new Response(JSON.stringify({ error: { name: "UNKNOWN_TRANSACTION" } }));
      return new Response(JSON.stringify({ result: chainAnswer(t, body.params.tx_hash === opts.tamper) }));
    }
    throw new Error("no live reads in this test");
  }) as unknown as typeof fetch;
  return { f, urls };
}

test("J5 archival RPCs are on the testnet allowlist; txOutcomeOf reads signer, method, outcome and events", () => {
  for (const u of ARCHIVAL_RPC) assertNotMainnet(u);
  const sale = data.steps["honest-sale"].txs[1];
  const o = txOutcomeOf(chainAnswer(sale));
  assert.equal(o.signer, sale.by);
  assert.equal(o.method, "ft_transfer_call");
  assert.ok(o.ok);
  assert.ok(o.events.includes("sale"));
});

test("J6 testnet check_claims: recorded transactions confirmed from archival nodes; a tampered one and missing live reads fail", async () => {
  const cfg = loadConfig({ AGENT_EXIT_NETWORK: "testnet", AGENT_EXIT_AGENTS: ["translator", "translator3", "cheat"].map((a) => `${a}.${data.root}`).join(",") });
  const { f, urls } = mockFetch();
  const r: { ok: boolean; body: any } = await new Tools(cfg, new TestnetBackend(cfg, f, null), schemas).call("check_claims", {});
  assert.equal(r.ok, true, JSON.stringify(r.body));
  const body: any = r.body;
  assert.equal(schemas.validateOutput.check_claims(body), true, JSON.stringify(schemas.validateOutput.check_claims.errors));
  const by = Object.fromEntries(body.claims.map((c: any) => [c.id, c]));
  for (const id of ["K4", "K6", "K7"]) assert.equal(by[id].pass, true, `${id}: ${by[id].observed}`);
  assert.match(by.K4.observed, /^2\/2 recorded transactions confirmed on chain$/);
  // The live reads were refused by the mock, so the live claims fail honestly.
  for (const id of ["K1", "K2", "K3", "K5"]) assert.equal(by[id].pass, false, id);
  assert.equal(body.pass, false);
  assert.ok(urls.every((u) => ARCHIVAL_RPC.includes(u) || cfg.rpc.includes(u)), "only allowlisted hosts");

  const sale = data.steps["honest-sale"].txs[1].tx;
  const t2 = mockFetch({ tamper: sale });
  const r2: { ok: boolean; body: any } = await new Tools(cfg, new TestnetBackend(cfg, t2.f, null), schemas).call("check_claims", {});
  const k4 = r2.body.claims.find((c: any) => c.id === "K4");
  assert.equal(k4.pass, false);
  assert.match(k4.observed, /1\/2 .*differs/);
});
