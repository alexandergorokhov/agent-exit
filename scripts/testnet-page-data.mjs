// web/data/testnet.json from the recorded testnet run (out/scenes/*.json):
// per demo step, each transaction with its outcome, the contract's error code
// and its NearBlocks link. Only what the runner recorded; nothing is invented.
import { readFileSync, writeFileSync, existsSync } from "node:fs";

const STEP_SCENE = { "break-in": 1, business: 2, verified: 3, cheat: 4, "honest-sale": 5, "agent-buys-agent": 6 };
const out = { schema: "web-testnet.v1", network: "testnet", money: "xUSD-FAKE", steps: {} };

function short(err) {
  if (!err) return null;
  if (/does not exist|AccessKeyNotFound/.test(err)) return "rejected by the node: the access key does not exist (no transaction possible)";
  const m = /(?:panicked: |PanicMsg\(")([^"\\)]+)/.exec(err);
  return m ? m[1].slice(0, 120) : err.slice(0, 120);
}

for (const [id, n] of Object.entries(STEP_SCENE)) {
  const f = `out/scenes/scene-${n}.json`;
  if (!existsSync(f)) continue;
  const s = JSON.parse(readFileSync(f, "utf8"));
  out.run ??= s.recorded_at || s.time || null;
  out.steps[id] = {
    scene: n,
    pass: s.pass === true,
    what: s.what,
    scan: s.scan ? { agent: s.scan.agent, verdict: s.scan.verdict, findings: (s.scan.findings || []).map((f) => ({ code: f.code, detail: f.detail })) } : null,
    // tx/by/receiver/method/events let the page re-check each stage against the chain ("Verify on chain").
    txs: (s.txs || []).map((t) => ({
      label: t.label, ok: t.ok === true, error: short(t.error), explorer: t.explorer || null,
      tx: t.tx || null, by: t.by || null, receiver: t.receiver || null, method: t.method || null,
      events: (t.events || []).map((e) => e.event),
    })),
  };
}
const dep = existsSync("out/testnet/deploy.json") ? JSON.parse(readFileSync("out/testnet/deploy.json", "utf8")) : {};
out.root = dep.root || null;
out.agent_code_hash = dep.agent_code_hash || null;
out.run = "2026-11-03";
writeFileSync("web/data/testnet.json", JSON.stringify(out, null, 2) + "\n");
console.log(`web/data/testnet.json: ${Object.keys(out.steps).length} steps, ${Object.values(out.steps).reduce((a, s) => a + s.txs.filter((t) => t.explorer).length, 0)} explorer links`);
