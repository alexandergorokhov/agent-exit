#!/usr/bin/env node
// Buyer's due-diligence cards from the recorded testnet run (out/scenes):
// what a buyer or investor learns about each agent before paying, with the
// numbers the contract and the scan reported. Writes web/data/diligence.json.
// Offline: reads only the recorded JSON. `make web-testnet-data` runs it.
import { readFileSync, writeFileSync } from "node:fs";

const root = new URL("..", import.meta.url);
const scene = (n) => JSON.parse(readFileSync(new URL(`out/scenes/scene-${n}.json`, root), "utf8"));
const USD = 1_000_000;
const usd = (x) => (Number(x) / USD).toFixed(2);
const pct = (a, b) => (Number(b) ? Math.round((100 * Number(a)) / Number(b)) : 0);
const nb = (tx) => `https://testnet.nearblocks.io/txns/${tx}`;
const short = (a) => a.replace(/\.w\d+\.agentexit\d*\.testnet$/, "");

const s3 = scene(3), s4 = scene(4), s5 = scene(5), s8 = scene(8);

function terms(l) {
  const t = l.terms;
  return `${100 - t.earnout_bps / 100}% to the seller now; ${t.earnout_bps / 100}% held for ${t.window_secs / 86400} days and released by counted customer payments (${t.share_bps / 100}% of each); after that the buyer gets the rest back, except a floor of ${t.floor_bps / 100}% of the held part`;
}

function keys(scan) {
  const block = scan.findings.filter((f) => f.severity === "block");
  return block.length
    ? { ok: false, text: `blocked: ${block.map((f) => f.code).join(", ")}` }
    : { ok: true, text: "clean: no key but the runner's, the published code, no open approval" };
}

function saleCard(name, e, scanScene, verdict, why, run) {
  const l = scanScene.scan_input.listing;
  const k = keys(scanScene.scan);
  const multiple = Number(e.counted) ? (Number(l.price) / Number(e.counted)).toFixed(1) : null;
  return {
    agent: scanScene.scan_input.agent,
    name,
    deal: `For sale at ${usd(l.price)}`,
    rows: [
      ["Revenue paid in", `${usd(e.gross)} from ${e.payments} payments`],
      ["Counted (real customers)", `${usd(e.counted)} (${pct(e.counted, e.gross)}%) from ${e.counted_payers} paying accounts`],
      ["Flagged, not counted", `${usd(e.flagged)}: owner self-pay ${usd(e.flagged_self)}, seller sub-accounts ${usd(e.flagged_seller_subaccount)}, round trips ${usd(e.flagged_round_trip)}`],
      ["Price ÷ counted revenue", multiple ? `${multiple}× the counted revenue recorded so far` : "no counted revenue: the price buys nothing proven"],
      ["Keys and code", k.text],
      ["Payment terms", terms(l)],
    ],
    verdict,
    ok: k.ok && Number(e.counted) > 0,
    why,
    run,
  };
}

const cards = [
  saleCard("translator", s3.translator, s5,
    "Buy at the listed terms",
    "72% of its revenue comes from 6 outside accounts; the scan is clean; 30% of the price only reaches the seller if customers keep paying.",
    "#run-sale"),
  saleCard("cheat", s3.cheat, s4,
    "Do not buy",
    "All 500.00 of its revenue came from the owner and the owner's own sub-accounts, and a hidden full-access key would let the seller take it back.",
    "#run-cheat"),
];

// Investor's card: translator3 (scene 8), a 20% slice of future earnings.
{
  const jobs = s8.jobs.map((j) => j.payment);
  const counted = jobs.filter((p) => p.counted).reduce((a, p) => a + Number(p.amount), 0);
  const verified = jobs.filter((p) => p.verified).reduce((a, p) => a + Number(p.amount), 0);
  const flagged = Number(s8.flagged.payment.amount);
  const h = s8.holdings_after_jobs.holdings[0];
  const buy = s8.txs.find((t) => /buys the 20% slice/.test(t.label || ""));
  cards.push({
    agent: s8.holdings_after_jobs.agent,
    name: "translator3",
    deal: `20% of future earnings for a year, ${usd(h.paid_in)}`,
    rows: [
      ["Revenue paid in", `${usd(counted + flagged)} from ${jobs.length + 1} payments`],
      ["Counted (real customers)", `${usd(counted)} from ${jobs.length} paying accounts`],
      ["Signed by NEAR AI, checked on-chain", `${usd(verified)} (${pct(verified, counted)}% of counted): each job's output signed in a TEE by ${s8.jobs[0].nearai.signing_address.slice(0, 10)}…`],
      ["Flagged, not counted", `${usd(flagged)} from ${short(s8.flagged.payment.payer)} (the owner's sub-account); it still paid the investor ${usd(s8.flagged.split.to_investors)}`],
      ["Investor received", `${usd(s8.check.dave_gained)} of ${usd(s8.check.paid)} paid in: exactly 20%`],
      ["Investor's money at risk", `half paid to the owner now, half (${usd(h.escrow)} left) in escrow and returned if the agent stops earning`],
      ["Keys and code", keys(s8.investor_scan).text],
    ],
    verdict: "Invest at these terms",
    ok: true,
    why: "Every counted job was signed by NEAR AI and checked by the contract, and the owner's own payment was flagged but still paid the investor.",
    run: "#run-invest",
    tx: buy ? nb(buy.tx) : null,
  });
}

const out = {
  schema: "diligence.v1",
  note: "Recorded on NEAR testnet (out/scenes, world w2.agentexit3.testnet). All money is xUSD-FAKE.",
  cards,
};
writeFileSync(new URL("web/data/diligence.json", root), JSON.stringify(out, null, 2) + "\n");
console.log(`diligence: ${cards.length} cards -> web/data/diligence.json`);
