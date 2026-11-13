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

test("J6 testnet check_claims: recorded transactions confirmed from archival nodes; a tampered or unknown one fails; reads with no RPC answer are not checked (pass null)", async () => {
  const cfg = loadConfig({ AGENT_EXIT_NETWORK: "testnet", AGENT_EXIT_AGENTS: ["translator", "translator3", "cheat"].map((a) => `${a}.${data.root}`).join(",") });
  const { f, urls } = mockFetch();
  const r: { ok: boolean; body: any } = await new Tools(cfg, new TestnetBackend(cfg, f, null), schemas).call("check_claims", {});
  assert.equal(r.ok, true, JSON.stringify(r.body));
  const body: any = r.body;
  assert.equal(schemas.validateOutput.check_claims(body), true, JSON.stringify(schemas.validateOutput.check_claims.errors));
  const by = Object.fromEntries(body.claims.map((c: any) => [c.id, c]));
  for (const id of ["K4", "K6", "K9"]) assert.equal(by[id].pass, true, `${id}: ${by[id].observed}`);
  assert.match(by.K4.observed, /^2\/2 recorded transactions confirmed on chain$/);
  // The live reads were refused by the mock (no RPC answered), so those claims
  // are not checked: never a pass, and never reported as a false claim.
  for (const id of ["K1", "K2", "K3", "K5"]) {
    assert.equal(by[id].pass, null, id);
    assert.equal(by[id].status, "unverified_network", id);
  }
  assert.notEqual(body.pass, true);
  assert.equal(body.status, "unverified_network");
  assert.equal(body.failed, 0);
  assert.equal(by.K9.world, "agentexit6.testnet");
  assert.equal(by.K4.world, data.root);
  assert.ok(urls.every((u) => ARCHIVAL_RPC.includes(u) || cfg.rpc.includes(u)), "only allowlisted hosts");

  const sale = data.steps["honest-sale"].txs[1].tx;
  const t2 = mockFetch({ tamper: sale });
  const r2: { ok: boolean; body: any } = await new Tools(cfg, new TestnetBackend(cfg, t2.f, null), schemas).call("check_claims", {});
  const k4 = r2.body.claims.find((c: any) => c.id === "K4");
  assert.equal(k4.pass, false);
  assert.equal(k4.status, "fail");
  assert.match(k4.observed, /1\/2 .*differs/);
  assert.equal(r2.body.pass, false);
  assert.equal(r2.body.status, "fail");

  // A transaction the node does not know is a failed claim, not a network problem.
  const t3 = mockFetch();
  const g = data.steps["nearai-check"].txs[0].tx;
  recorded.delete(g);
  try {
    const r3: { ok: boolean; body: any } = await new Tools(cfg, new TestnetBackend(cfg, t3.f, null), schemas).call("check_claims", {});
    const k9 = r3.body.claims.find((c: any) => c.id === "K9");
    assert.equal(k9.status, "fail", k9.observed);
    assert.equal(k9.pass, false);
  } finally {
    recorded.set(g, data.steps["nearai-check"].txs[0]);
  }

  // No RPC at all (a sandboxed judge): every claim is not checked (pass null), never false.
  const down = (async () => {
    throw new TypeError("fetch failed");
  }) as unknown as typeof fetch;
  const r4: { ok: boolean; body: any } = await new Tools(cfg, new TestnetBackend(cfg, down, null), schemas).call("check_claims", {});
  assert.equal(r4.ok, true, JSON.stringify(r4.body));
  assert.equal(schemas.validateOutput.check_claims(r4.body), true, JSON.stringify(schemas.validateOutput.check_claims.errors));
  assert.ok(r4.body.claims.length >= 7);
  for (const c of r4.body.claims) {
    assert.equal(c.status, "unverified_network", `${c.id}: ${c.observed}`);
    assert.equal(c.pass, null, c.id);
  }
  assert.equal(r4.body.pass, null);
  assert.equal(r4.body.status, "network_unreachable");
  assert.equal(r4.body.failed, 0);
  assert.match(r4.body.note, /no RPC reachable: nothing on testnet was checked/);
  assert.match(r4.body.note, /make judge/);
});

test("J7 (review 6, L4) a live read that answered with a failing condition is a fail, even when another input had no RPC answer", async () => {
  const cfg = loadConfig({ AGENT_EXIT_NETWORK: "testnet", AGENT_EXIT_AGENTS: ["translator", "translator3", "cheat"].map((a) => `${a}.${data.root}`).join(",") });
  const brk = data.steps["break-in"].txs.find((t: any) => t.tx && t.method).tx;
  const inv = data.steps["invest"].txs.find((t: any) => t.tx && t.method && /pays translator3/.test(String(t.label))).tx;
  const { f: base } = mockFetch();
  // One break-in tx and one invest tx are unreachable; everything else answers.
  const f = (async (url: string, init: any) => {
    const body = JSON.parse(init.body);
    if (body.method === "tx" && (body.params.tx_hash === brk || body.params.tx_hash === inv)) throw new TypeError("fetch failed");
    return base(url, init);
  }) as unknown as typeof fetch;
  const t = new Tools(cfg, new TestnetBackend(cfg, f, null), schemas);
  const orig = t.call.bind(t);
  // Live reads answer, but with the claim's condition false.
  (t as any).call = async (name: string, args: any) => {
    if (name === "verify_lockout") return { ok: true, body: { locked_out: false } };
    if (name === "get_earnings") return { ok: true, body: { earnings: { counted: "10.00", verified: "0.00", flagged: "0.00" } } };
    return orig(name as any, args);
  };
  const r: { ok: boolean; body: any } = await t.call("check_claims", {});
  assert.equal(r.ok, true, JSON.stringify(r.body));
  const by = Object.fromEntries(r.body.claims.map((c: any) => [c.id, c]));
  assert.equal(by.K5.status, "fail", by.K5.observed);
  assert.equal(by.K5.pass, false);
  assert.equal(by.K1.status, "fail", by.K1.observed);
  assert.equal(r.body.status, "fail");
});
