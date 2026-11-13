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
// The world is the account the agents live under, e.g. agentexit6.testnet.
const worldOf = (cards) => (cards[0]?.agent || "").split(".").slice(-2).join(".") || "unknown";
const short = (a) => a.replace(/(\.factory)?(\.w\d+)?\.agentexit\d*\.testnet$/, "");

const s3 = scene(3), s4 = scene(4), s5 = scene(5), s8 = scene(8);
// Scene 12 (optional): the open offer anyone can buy into, on the scene-8 agent.
let s12 = null;
try { s12 = scene(12); } catch { /* not recorded */ }
// Today's totals for the open-offer agent (page data, written just before this runs): the card's headline numbers match the market table.
let now = null;
try { now = JSON.parse(readFileSync(new URL("web/data/testnet.json", root), "utf8")).open_offer?.earnings || null; } catch { /* first run */ }
const money = (x) => Number(usd(x)).toLocaleString("en-US", { minimumFractionDigits: 2, maximumFractionDigits: 2 });

function terms(l) {
  const t = l.terms;
  if (t.earnout_bps === 10_000) return `pay from revenue: nothing to the seller now; the whole price is held for ${t.window_secs / 86400} days and released by counted customer payments (${t.share_bps / 100}% of each); after that the buyer gets the rest back, except a floor of ${t.floor_bps / 100}% of the held part`;
  return `${100 - t.earnout_bps / 100}% to the seller now; ${t.earnout_bps / 100}% held for ${t.window_secs / 86400} days and released by counted customer payments (${t.share_bps / 100}% of each); after that the buyer gets the rest back, except a floor of ${t.floor_bps / 100}% of the held part`;
}

// What faking this revenue would have cost the seller (S10, PR5). Before a
// sale, payments from wallets the meter cannot link come back to the seller
// as revenue: almost free. After the sale each 1.00 of escrow they pull out
// with such payments costs 10000/share_bps (2.00 at 50%), so it never pays;
// with the whole price held ("pay from revenue") nothing at all is paid on
// the strength of the history.
function costToFake(l) {
  const t = l.terms;
  const per = (10_000 / t.share_bps).toFixed(2);
  const now = (100 - t.earnout_bps / 100);
  const base = `before the sale: about 0 (payments from unlinked wallets come back to the seller as revenue, so weigh new payers down); after the sale: ${per} paid in for each 1.00 of escrow released, so faking then always loses money`;
  return now === 0
    ? `${base}. Pay from revenue: the whole price is held, so a faked history earns the seller nothing up front`
    : `${base}. Only the ${now}% paid now (${usd(Number(l.price) * now / 100)}) rests on the history alone; listing with the whole price held removes even that`;
}

function keys(scan) {
  const block = scan.findings.filter((f) => f.severity === "block");
  return block.length
    ? { ok: false, text: `blocked: ${block.map((f) => f.code).join(", ")}` }
    : { ok: true, text: "clean: no key but the runner's, the published code, no open approval" };
}

// Payer age and spread (N3), the same rule as the MCP get_earnings
// revenue_quality: payments from payers first seen in the last 7 days count half.
function quality(e) {
  const counted = Number(e.counted), fresh = Number(e.counted_from_fresh_payers ?? 0);
  const weighted = counted - fresh / 2;
  const warn = [];
  if (counted && fresh * 2 > counted) warn.push("most of it is from payers first seen in the last 7 days");
  if (e.counted_payers && e.counted_payers < 10) warn.push(`only ${e.counted_payers} paying accounts`);
  if (e.days_total < 7) warn.push(`${e.days_total} day(s) of history`);
  return { weighted, warn, text: `${e.counted_payers} paying accounts over ${e.days_total} day(s); ${pct(fresh, counted)}% of counted revenue from payers first seen in the last 7 days, so it weighs ${usd(weighted)} (new payers count half)${warn.length ? `. Weigh it down: ${warn.join("; ")}` : ""}` };
}

// role: the one-line part each agent plays on the page (one cast: the same names on the Exchange board).
const ROLE = { translator: "The sold agent", cheat: "The cheat", translator3: "Open to investors" };

function saleCard(name, e, scanScene, verdict, why, run, short) {
  const l = scanScene.scan_input.listing;
  const k = keys(scanScene.scan);
  // K8: a plain deploy (the cheat) cannot be listed at all: the contract refuses (NotBornClean).
  const refused = !l;
  const multiple = !refused && Number(e.counted) ? (Number(l.price) / Number(e.counted)).toFixed(1) : null;
  return {
    agent: scanScene.scan_input.agent,
    name,
    role: ROLE[name],
    deal: refused ? "Not for sale: the contract refused to list it (NotBornClean)" : `For sale at ${usd(l.price)}`,
    rows: [
      ["Revenue paid in", `${usd(e.gross)} from ${e.payments} payments`],
      ["Counted (real customers)", `${usd(e.counted)} (${pct(e.counted, e.gross)}%) from ${e.counted_payers} paying accounts`],
      ["Flagged, not counted", `${usd(e.flagged)}: owner self-pay ${usd(e.flagged_self)}, seller sub-accounts ${usd(e.flagged_seller_subaccount)}, round trips ${usd(e.flagged_round_trip)}`],
      ["Payer age and spread", Number(e.counted) ? quality(e).text : "no counted revenue to weigh"],
      ["Price ÷ counted revenue", refused ? "no price: it cannot be listed" : multiple ? `${multiple}× the counted revenue recorded so far (${(Number(l.price) / quality(e).weighted).toFixed(1)}× the age-weighted figure)` : "no counted revenue: the price buys nothing proven"],
      ["Keys and code", k.text],
      ["Payment terms", refused ? "none: list_for_sale was refused NotBornClean (a plain deploy, not created by the born-clean factory, may hold a key no contract can list), and a buy sent anyway is refunded" : terms(l)],
      ["Cost to fake this revenue", refused ? "not applicable: nothing can be sold" : costToFake(l)],
    ],
    verdict,
    ok: k.ok && Number(e.counted) > 0,
    why,
    run,
    // The verdict-first summary the page shows before "Full due diligence".
    short,
    // translator was then sold to bob (scene 5); the cheat never was.
    deal_short: refused ? "Refused: NotBornClean" : name === "translator" ? `Sold to bob for ${money(l.price)}` : `For sale at ${money(l.price)}`,
    metrics: [
      ["Real revenue", `${usd(e.counted)} (${pct(e.counted, e.gross)}%)`],
      ["Self-paid", usd(e.flagged)],
      ["Keys", k.ok ? "clean" : "hidden full-access key"],
      ["Price ÷ revenue", multiple ? `${multiple}×` : "nothing proven"],
    ],
    bar: { real: Number(usd(e.counted)), self: Number(usd(e.flagged)) },
  };
}

const cards = [
  saleCard("translator", s3.translator, s5,
    "Passes the buyer's checks",
    "72% of its revenue comes from 6 outside accounts and the scan is clean, but the customers are new, so weigh the revenue down; 30% of the price only reaches the seller if customers keep paying.",
    "#run-sale",
    "Pass: 72% from 6 real customers, no hidden keys. Customers are new."),
  s4.scan_input.listing
    // The world recorded before the born-clean rule (K8): the cheat was listed and the scan blocked it.
    ? saleCard("cheat", s3.cheat, s4,
      "Blocked before any money moves",
      "All 500.00 of its revenue came from the owner and the owner's own sub-accounts, and a hidden full-access key would let the seller take it back.",
      "#run-cheat",
      "Blocked: all 500 was self-paid, and a hidden key lets her take it back.")
    // After the rule: a plain deploy cannot be listed; the contract refuses (NotBornClean).
    : saleCard("cheat", s3.cheat, s4,
      "Refused by the contract and blocked by the scan",
      "All 500.00 of its revenue came from the owner and the owner's own sub-accounts, and a hidden full-access key would let the seller take it back. It is a plain deploy, not created by the factory, so the contract itself refuses to list it.",
      "#run-cheat",
      "Refused: it was deployed with a key, so the contract will not list it. All 500 was self-paid and a hidden key lets her take it back."),
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
    role: ROLE.translator3,
    deal: `20% of future earnings for a year, ${usd(h.paid_in)}`,
    rows: [
      ...(now ? [["Now", `${now.counted} real revenue (${now.verified} of it signed by NEAR AI), ${now.flagged} self-paid, ${now.gross} paid in. Later payments were added after the first investment below.`]] : []),
      ["At the first investment", "the 20% sale, before those later payments."],
      ["Revenue paid in then", `${usd(counted + flagged)} from ${jobs.length + 1} payments`],
      ["Counted then (real customers)", `${usd(counted)} from ${jobs.length} paying accounts`],
      ["Signed by NEAR AI then, checked on-chain", `${usd(verified)} (${pct(verified, counted)}% of counted): each job's output signed in a TEE by ${s8.jobs[0].nearai.signing_address.slice(0, 10)}…`],
      ["Flagged, not counted", `${usd(flagged)} from ${short(s8.flagged.payment.payer)} (the owner's sub-account); it still paid the investor ${usd(s8.flagged.split.to_investors)}`],
      ["Investor received", `${usd(s8.check.dave_gained)} = 20% of all ${usd(s8.check.paid)} paid in. The payout is on all money paid in, self-paid too; the valuation uses only real revenue.`],
      ["Investor's money at risk", `half paid to the owner now, half (${usd(h.escrow)} left) in escrow and returned if the agent stops earning`],
      ["Keys and code", keys(s8.investor_scan).text],
    ],
    verdict: "Passes the investor's checks",
    ok: true,
    why: "At the first investment every counted job was signed by NEAR AI, and the owner's own payment was flagged but still paid the investor.",
    run: "#run-invest",
    tx: buy ? nb(buy.tx) : null,
    short: now ? `Pass: ${now.verified} of ${now.counted} signed by NEAR AI; first investor paid to the cent.` : "Pass: every counted job signed by NEAR AI; earlier investor paid to the cent.",
    // dave's 20% stays in the proof scenario; the card points at the offer a visitor can buy.
    deal_short: s12 && s12.agent === s8.holdings_after_jobs.agent ? `Open now: ${Number(s12.buy_part.part_bps) / 100}% parts, ${usd(s12.buy_part.amount)} each` : "20% sold to an investor",
    deal_link: s12 && s12.agent === s8.holdings_after_jobs.agent ? "#invest-yourself" : null,
    metrics: [
      ["Real revenue", now ? `${now.counted} (${now.verified} NEAR AI)` : `${usd(counted)} (${pct(verified, counted)}% NEAR AI)`],
      ["Self-paid", now ? now.flagged : usd(flagged)],
      ["Keys", keys(s8.investor_scan).ok ? "clean" : "hidden full-access key"],
      ["First investor got", `${usd(s8.check.dave_gained)} (20% of ${usd(s8.check.paid)} then)`],
    ],
    bar: now ? { real: Number(now.counted), self: Number(now.flagged) } : { real: Number(usd(counted)), self: Number(usd(flagged)) },
    // Days with counted revenue (the contract's day index): the page calls a multiple on so little history early.
    revenue_days: new Set(jobs.filter((p) => p.counted).map((p) => p.day)).size,
  });
}

const out = {
  schema: "diligence.v1",
  note: `Recorded on NEAR testnet (out/scenes, world ${worldOf(cards)}). All money is xUSD-FAKE, a test token.`,
  cards,
};
writeFileSync(new URL("web/data/diligence.json", root), JSON.stringify(out, null, 2) + "\n");
console.log(`diligence: ${cards.length} cards -> web/data/diligence.json`);
