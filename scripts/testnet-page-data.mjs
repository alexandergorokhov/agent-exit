// web/data/testnet.json from the recorded testnet run (out/scenes/*.json):
// per demo step, each transaction with its outcome, the contract's error code
// and its NearBlocks link. Only what the runner recorded; nothing is invented.
import { readFileSync, writeFileSync, existsSync } from "node:fs";

const STEP_SCENE = { "break-in": 1, business: 2, verified: 3, cheat: 4, "honest-sale": 5, "agent-buys-agent": 6, invest: 8, "born-clean": 9, "sealed-prompt": 10, "business-day": 11 };
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
// Gate G6: a real NEAR AI Cloud signature checked by the contract.
if (existsSync("out/gates/g6.json")) {
  const g = JSON.parse(readFileSync("out/gates/g6.json", "utf8"));
  out.steps["nearai-check"] = {
    scene: "G6",
    pass: g.pass === true,
    what: "NEAR AI Cloud answers a customer's request inside its TEE and signs model:sha256(request):sha256(response). The registry admin adds the published signing address; the customer pays with the signature; the contract recovers the signer with ecrecover and marks the payment verified.",
    scan: null,
    txs: (g.txs || []).map((t) => ({
      label: t.label, ok: t.ok === true, error: short(t.error), explorer: t.explorer || null,
      tx: t.tx || null, by: t.by || null, receiver: t.receiver || null, method: t.method || null,
      events: (t.events || []).map((e) => e.event),
    })),
  };
}
// A real AI model (Claude Haiku) buying through the MCP server on testnet.
if (existsSync("out/llm-buyer-testnet.json")) {
  const l = JSON.parse(readFileSync("out/llm-buyer-testnet.json", "utf8"));
  const sm = l.summary || {};
  const root = (l.testnet && l.testnet.root) || "agentexit3.testnet";
  out.steps["ai-buyer"] = {
    scene: "AI",
    pass: sm.pass === true,
    what: `${(l.models_reported || [l.model_requested])[0]} read the Agent Exit skill and used the MCP tools on testnet: its scan of cheat said ${(sm.scanned_cheat || []).join(", ")}, so it did not buy it; translator scanned ${(sm.scanned_translator || []).join(", ")} and it bought it, then checked the seller is locked out (${sm.seller_locked_out ? "yes" : "no"}). ${sm.steps} model steps, ${l.usage ? l.usage.total_tokens : "?"} tokens. The server, not the model, holds the key and enforces max_price, confirm and the spend cap.`,
    scan: null,
    txs: ((l.testnet && l.testnet.txs) || []).filter((t) => t.tx_hash).map((t) => ({
      label: `the AI's ${t.tool}: ${l.buyer} pays for translator (one ft_transfer_call)`, ok: t.ok === true, error: null,
      explorer: t.explorer || null, tx: t.tx_hash, by: l.buyer, receiver: `xusd.${root}`, method: "ft_transfer_call", events: [],
    })),
  };
}
const dep = existsSync("out/testnet/deploy.json") ? JSON.parse(readFileSync("out/testnet/deploy.json", "utf8")) : {};
out.root = dep.root || null;
out.agent_code_hash = dep.agent_code_hash || null;
out.run = new Date().toISOString().slice(0, 10); // the day this page data was made from out/
// Scene 13: the Exchange board (factory-born agents with their own revenue mix).
if (existsSync("out/scenes/scene-13.json")) {
  const x = JSON.parse(readFileSync("out/scenes/scene-13.json", "utf8"));
  if (x.pass && Array.isArray(x.board)) {
    out.exchange = { factory: x.factory, agents: x.board.map((b) => ({
      agent: b.agent, name: b.name, status: b.status, born_clean: b.born_clean === true,
      gross: String(b.earnings?.gross ?? 0), counted: String(b.earnings?.counted ?? 0), verified: String(b.earnings?.verified ?? 0),
      flagged: String(b.earnings?.flagged ?? 0), payers: Number(b.earnings?.counted_payers ?? 0),
      ask: b.listing?.price ? String(b.listing.price) : null,
      offers: (Array.isArray(b.shares) ? b.shares : []).map((o) => ({ offer_id: Number(o.offer_id), min_part: Number(o.min_part), bps: Number(o.bps), sold_bps: Number(o.sold_bps ?? 0), price: String(o.price), end: String(o.end) })),
    })) };
  }
}
// Scene 12: an open offer anyone can buy into from the page ("Invest yourself").
if (existsSync("out/scenes/scene-12.json")) {
  const o = JSON.parse(readFileSync("out/scenes/scene-12.json", "utf8"));
  if (o.pass && o.offer && o.buy_part) {
    out.open_offer = { agent: o.agent, xusd: `xusd.${dep.root}`, offer: o.offer, part_bps: o.buy_part.part_bps, amount: o.buy_part.amount, msg: o.buy_part.msg };
    // The Invest button's own transaction, signed by script for an outside
    // account (scripts/testnet/invest-button.mjs -> out/invest-button-testnet.json).
    if (existsSync("out/invest-button-testnet.json")) {
      const b = JSON.parse(readFileSync("out/invest-button-testnet.json", "utf8"));
      if (b.pass && b.agent === o.agent && /^[1-9A-HJ-NP-Za-km-z]{43,44}$/.test(b.tx_hash)) {
        out.open_offer.example_tx = { account: b.account, tx: b.tx_hash, explorer: b.explorer, holdings_bps: b.holdings_bps_after, note: b.note };
      }
      // The agent's earnings when the open offer was bought into (the
      // investor agent's own get_earnings read, out/agent-invests-testnet.json).
      if (existsSync("out/agent-invests-testnet.json")) {
        const inv = JSON.parse(readFileSync("out/agent-invests-testnet.json", "utf8"));
        const e = (inv.steps || []).map((x) => x.response && x.response.earnings).find(Boolean);
        if (e) out.open_offer.earnings = { gross: e.gross, counted: e.counted, verified: e.verified, flagged: e.flagged, payments: e.payments, days: Array.isArray(e.days) ? e.days.length : null, source: "out/agent-invests-testnet.json" };
      }
    }
  }
}
writeFileSync("web/data/testnet.json", JSON.stringify(out, null, 2) + "\n");
console.log(`web/data/testnet.json: ${Object.keys(out.steps).length} steps, ${Object.values(out.steps).reduce((a, s) => a + s.txs.filter((t) => t.explorer).length, 0)} explorer links`);
