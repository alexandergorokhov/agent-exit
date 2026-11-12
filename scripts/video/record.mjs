#!/usr/bin/env node
// Records the demo video (about 2 minutes) from the page, offline: the recorded
// testnet data (web/data) and the playground (the contract core compiled to
// wasm). 1920x1080 with the page zoomed, so small panels stay readable.
// No network, no wallet. Captions are drawn on the page. The "Run and verify"
// moment replays the recorded transactions through the page's own checker
// (this machine has no chain access); the caption says so.
//   node web/serve.mjs 8090 &   then   node scripts/video/record.mjs
// Writes out/video/raw/*.webm; scripts/video/encode.sh makes the mp4 + GIF.
import { createRequire } from "node:module";
import { mkdirSync, rmSync, existsSync, readFileSync, writeFileSync } from "node:fs";
import { speech } from "./speech.mjs";
import { LINES } from "./script.mjs";
const require = createRequire(import.meta.url);
const { chromium } = require("playwright");

const URL_ = process.env.PAGE_URL || "http://127.0.0.1:8090/";
const OUT = new URL("../../out/video/raw/", import.meta.url).pathname;
rmSync(OUT, { recursive: true, force: true });
mkdirSync(OUT, { recursive: true });

const W = 1920, H = 1080;
const browser = await chromium.launch();
const ctx = await browser.newContext({ viewport: { width: W, height: H }, recordVideo: { dir: OUT, size: { width: W, height: H } } });
// "Run and verify" asks the chain for each recorded transaction through the page's relay.
// There is no chain access here, so the answers are built from the recorded run itself
// (web/data/testnet.json), the same way web/test/smoke.mjs does, and the page's own checker compares them.
const rec = JSON.parse(readFileSync(new URL("../../web/data/testnet.json", import.meta.url), "utf8"));
const byHash = new Map(Object.values(rec.steps).flatMap((s) => s.txs || []).filter((t) => t.tx).map((t) => [t.tx, t]));
const cheatKey = /(ed25519:\w+)/.exec(rec.steps.cheat.scan.findings[0].detail)[1];
await ctx.route("**/api/rpc", async (route) => {
  const j = JSON.parse(route.request().postData());
  let result;
  if (j.method === "tx") {
    const t = byHash.get(j.params.tx_hash);
    const logs = t.events.map((e) => `EVENT_JSON:${JSON.stringify({ standard: "agent_exit", event: e, data: [] })}`);
    result = { status: t.ok ? { SuccessValue: "" } : { Failure: { ActionError: { kind: { FunctionCallError: { ExecutionError: `Smart contract panicked: ${t.error}` } } } } },
      transaction: { signer_id: t.by, receiver_id: t.receiver, actions: [{ FunctionCall: { method_name: t.method } }] },
      transaction_outcome: { block_hash: "(replayed from the recorded run)" }, receipts_outcome: [{ outcome: { logs, status: t.ok ? { SuccessValue: "" } : { Failure: {} } } }] };
  } else {
    const fc = { public_key: "ed25519:runner", access_key: { permission: { FunctionCall: {} } } };
    result = { keys: j.params.account_id.startsWith("cheat.") ? [{ public_key: cheatKey, access_key: { permission: "FullAccess" } }, fc] : [fc] };
  }
  await new Promise((r) => setTimeout(r, 260));
  return route.fulfill({ contentType: "application/json", body: JSON.stringify({ jsonrpc: "2.0", id: j.id, result }) });
});
const p = await ctx.newPage();
const wait = (ms) => p.waitForTimeout(ms);
let ZOOM = 1.25;
const zoom = async (z) => { ZOOM = z; await p.evaluate((z) => { document.documentElement.style.zoom = String(z); }, z); };
// Narration (optional): out/video/voice/manifest.json from voice.mjs. Each line
// starts when its caption shows and the next caption waits for it to end; the
// start times go to out/video/voice-track.json for encode.sh to mix in.
const V0 = Date.now(); // the recording starts when the page opens
let T0 = V0;
const VOICE = new URL("../../out/video/voice/manifest.json", import.meta.url).pathname;
const manifest = existsSync(VOICE) ? JSON.parse(readFileSync(VOICE, "utf8")) : null;
const track = [];
let speechEnd = 0;
const quiet = async () => { const left = speechEnd - Date.now(); if (left > 0) await wait(left); };
function say(text) {
  const v = manifest && text && manifest[text];
  if (!v) return 0;
  track.push({ t: (Date.now() - T0) / 1000, file: v.file, text });
  speechEnd = Date.now() + v.dur * 1000 + 300;
  return v.dur * 1000 + 300;
}

async function caption(text, ms = 1200, line = null, big = false) {
  await quiet();
  await p.evaluate(([t, big]) => {
    let c = document.getElementById("vid-cap");
    if (!c) {
      c = document.createElement("div");
      c.id = "vid-cap";
      c.style.cssText = "position:fixed;inset:0 0 auto 0;margin:0;border:0;width:auto;z-index:99999;background:rgba(10,20,16,.94);color:#fff;font:600 26px/1.3 Inter,system-ui,sans-serif;padding:14px 40px;text-align:center;box-shadow:0 4px 16px rgba(0,0,0,.25)";
      c.setAttribute("popover", "manual"); // the top layer, so an open agent panel's backdrop does not dim it
      document.body.appendChild(c);
    }
    // An open agent panel sits in the top layer: keep the caption beside it, not under it.
    const d = document.querySelector("dialog[open]");
    c.style.right = d ? `${d.offsetWidth}px` : "0";
    c.style.fontSize = big ? "34px" : "26px";
    c.textContent = t;
    c.style.display = t ? "block" : "none";
    if (c.matches(":popover-open")) c.hidePopover();
    if (t) c.showPopover();
  }, [text, big]);
  const spoken = line ? say(speech(line)) : 0;
  if (ms) await wait(Math.max(ms, spoken));
}
// A closing card: the logo flashes for a second, then the video ends.
async function card(lines, ms) {
  await quiet();
  await p.evaluate((lines) => {
    let c = document.getElementById("vid-card");
    if (!c) {
      c = document.createElement("div");
      c.id = "vid-card";
      c.style.cssText = "position:fixed;inset:0;z-index:100000;background:#000;color:#fff;display:flex;flex-direction:column;align-items:center;justify-content:center;gap:18px;font:600 30px/1.35 Inter,system-ui,sans-serif;text-align:center;padding:40px";
      document.body.appendChild(c);
    }
    c.replaceChildren(...lines.map((l, i) => {
      if (l.startsWith("img:")) { const im = document.createElement("img"); im.src = l.slice(4); im.alt = ""; im.style.cssText = "width:520px;border-radius:24px"; return im; }
      const d = document.createElement("div"); d.textContent = l; if (i === 1) d.style.cssText = "font-size:48px;font-weight:800"; return d;
    }));
    c.style.display = lines.length ? "flex" : "none";
  }, lines);
  if (ms) await wait(ms);
}
// Bring an element into view, instantly (smooth scrolling and page zoom do not mix well).
async function show(sel, block = "start") {
  await p.evaluate(([s, b]) => { const e = document.querySelector(s); if (e) e.scrollIntoView({ block: b, behavior: "instant" }); }, [sel, block]);
  await wait(150);
}
// Press a mission step, then show its result: the newest line of the feed, with the state panel above it when it fits.
async function act(k, ms = 900, view = "state") {
  await p.locator(`#mission-steps button[data-action="${k}"]`).click();
  await wait(250);
  if (view === "feed") { await show("#pg-feed", "start"); await p.evaluate(() => window.scrollBy(0, -170)); }
  else { await show("#missions", "start"); await p.evaluate(() => window.scrollBy(0, -110)); }
  await wait(ms);
}
// Spotlight one element for a moment: a green ring and a slight lift.
async function glow(sel, ms = 0) {
  await p.evaluate((s) => {
    document.querySelectorAll("[data-glow]").forEach((e) => { e.style.boxShadow = ""; e.style.transform = ""; e.removeAttribute("data-glow"); });
    const e = s && document.querySelector(s);
    if (!e) return;
    e.setAttribute("data-glow", "");
    e.style.transition = "box-shadow .25s, transform .25s";
    e.style.boxShadow = "0 0 0 4px #00ec97, 0 0 40px rgb(0 236 151 / .45)";
    e.style.transform = "scale(1.03)";
  }, sel);
  if (ms) await wait(ms);
}
const openAgent = async (name, ms) => { await p.click(`.mk-deed[data-agent="${name}"]`); await wait(400 + ms); };
const closeAgent = async () => { await p.keyboard.press("Escape"); await wait(400); };
async function home() {
  await quiet();
  await p.goto(URL_);
  await p.waitForSelector("#market[data-ready]");
  await p.waitForSelector('#play[data-ready="1"]');
  await zoom(1.25);
  await wait(300);
}

// Shot 1: the hook, on the market page in the first second: Alice, Polyglot, the question.
await home();
T0 = Date.now();
const trim = (T0 - V0) / 1000; // the page load before the first caption; encode.sh cuts it
await caption("Alice built Polyglot. She wants to sell it. Can a buyer trust the revenue?", 800, LINES.hook, true);
await caption("Fake revenue. Hidden keys. Buying an agent today is blind trust.", 800, LINES.problem);
await show("#market", "start");
await p.evaluate(() => window.scrollBy(0, -140));
await caption("Every agent carries a verdict, counted from who actually paid", 800, LINES.market);

// Shot 2: the verdicts a buyer sees before paying.
await p.click('#filters button[data-f="flag"]');
await openAgent("pump-bot", 0);
await glow(".mk-panel .kpis");
await caption("TrendPulse (ill-intended test): 400 of 410 is self-paid", 800, LINES.pump);
await closeAgent();
await openAgent("cheat", 0);
await glow(".mk-panel .mk-bad");
await caption("QuickQuote (test): the scan catches the key. New rule: the contract refuses to list a keyed agent (K8, offline)", 800, LINES.cheat);
await closeAgent();
await p.click('#filters button[data-f="sold"]');
await openAgent("translator", 0);
await glow(".mk-panel .mk-sold");
await caption("Polyglot: sold in one transaction; 6 take-backs refused, old key gone", 800, LINES.sold);
await closeAgent();

// Shot 3: check it, on the proof page: Run and verify ticks through the recorded testnet transactions.
await caption("", 0);
await p.goto(URL_ + "proof.html#run-sale");
await p.waitForSelector('#scenarios[data-ready="1"]');
await zoom(1.5);
await wait(600);
await show('[data-run="sale"]', "start");
await p.evaluate(() => window.scrollBy(0, -260));
await caption("Run and verify: each recorded testnet transaction, checked step by step (answers replayed in this recording)", 0, LINES.evidence);
await wait(1200);
await p.locator('[data-run="sale"]').click();
await wait(1500);
await show('[data-run="sale"]', "start");
await p.evaluate(() => window.scrollBy(0, 330));
await wait(2200);
await show('[data-run="sale"]', "start");
await p.evaluate(() => window.scrollBy(0, 1080));
await wait(1800);

// Shot 4: invest, and AI buyers (back on the market page).
await home();
await p.click('#filters button[data-f="all"]');
await openAgent("translator3", 0);
await p.fill("#mk-account", "judge.testnet");
await glow("#mk-invest");
await caption("Invest: 2.5% of Lingo Live's earnings for 25 test dollars", 800, LINES.invest);
await closeAgent();
await openAgent("research-desk", 0);
await glow(".mk-panel pre.mcp");
await caption("AI agents can buy too, over MCP", 800, LINES.mcp);
await glow(null);
await closeAgent();

// Shot 5: the playground, about 35 seconds: the page zoomed so the changing panel reads at 1080p.
await zoom(1.5);
await show("#missions", "start");
await p.evaluate(() => window.scrollBy(0, -110));
await p.locator('#missions button[data-mission="invest"]').click();
await wait(300);
await caption("The same contract code, run in your browser: alice sells 20% to dave", 0, LINES.playInvest);
await act("offer_share", 900);
await act("buy_share", 1000);
await caption("Each payment pays dave first, even alice's fake one", 0, LINES.playPay);
await act("pay_customer_inv", 1000);
await act("pay_self_inv", 1200, "feed");

await quiet();
await show("#missions", "start");
await p.evaluate(() => window.scrollBy(0, -110));
await p.locator('#missions button[data-mission="honest"]').click();
await wait(300);
await caption("One transaction: bob pays, alice's keys are deleted", 0, LINES.playBuy);
await act("list", 600);
await act("scan", 600);
await act("buy", 1200);
await caption("alice tries to get back in", 0, LINES.breakin);
await act("withdraw", 700, "feed");
await act("set_key", 700, "feed");
await act("old_key", 900, "feed");

await quiet();
await show("#missions", "start");
await p.evaluate(() => window.scrollBy(0, -110));
await p.locator('#missions button[data-mission="brain"]').click();
await wait(300);
await caption("The prompt is sealed by its hash; a cheaper one is delivered; bob is refunded in full", 0, LINES.brain);
await act("commit_config", 600);
await act("list", 500);
await act("scan", 500);
await act("buy", 600);
await act("pay_customer", 500);
await act("deliver_fake", 700);
await act("open_config", 700);
await act("dispute_config", 1400, "feed");

// Shot 6: what is new, the tagline, and a one-second logo flash.
await home();
await caption("Code, not trust, locks the seller out. An agent that ever had a key can't be listed.", 800, LINES.whyNew, true);
await caption("Buy AI agents by what they really earn. Live on NEAR testnet, test dollars only.", 800, LINES.close, true);
await caption("", 0);
await card(["img:media/logo.png", "Buy AI agents by what they really earn."], 1200);

await quiet();
await ctx.close();
await browser.close();
writeFileSync(new URL("../../out/video/trim.txt", import.meta.url).pathname, trim.toFixed(2) + "\n");
if (manifest) writeFileSync(new URL("../../out/video/voice-track.json", import.meta.url).pathname, JSON.stringify(track, null, 2) + "\n");
console.log(`video: raw recording in ${OUT}`);
