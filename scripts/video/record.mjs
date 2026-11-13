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
      c.style.cssText = "position:fixed;inset:0 0 auto 0;margin:0;border:0;width:auto;z-index:99999;background:#0a1410;color:#fff;font:600 26px/1.3 Inter,system-ui,sans-serif;padding:14px 40px;text-align:center;box-shadow:0 4px 16px rgba(0,0,0,.25)";
      c.setAttribute("popover", "manual"); // the top layer, so an open agent panel's backdrop does not dim it
      document.body.appendChild(c);
    }
    // An open agent panel sits in the top layer: keep the caption beside it, not under it.
    const d = document.querySelector("dialog[open]");
    c.style.right = d ? `${d.offsetWidth}px` : "0";
    c.style.fontSize = big ? "34px" : "26px";
    c.textContent = t;
    document.body.style.paddingTop = t ? "76px" : ""; // the page starts below the caption bar, so no heading is clipped under it
    c.style.display = t ? "block" : "none";
    if (c.matches(":popover-open")) c.hidePopover();
    if (t) c.showPopover();
  }, [text, big]);
  const spoken = line ? say(speech(line)) : 0;
  if (ms) await wait(Math.max(ms, spoken));
}
// Full-screen scenes (the opening counter, the lock-out climax, the end card), built from a small
// spec with DOM calls and the Web Animations API, because the page's strict CSP blocks inline styles
// in markup. N(css, children|text, opts): opts.at = seconds after the scene starts (fade and rise in),
// opts.stamp = a stamp that slams in, opts.grow = a bar that grows from the left, opts.counts = number
// counters [{from, to, at, dur, pre}], opts.color = [[at, "#hex"]] colour changes.
const N = (s, c = [], o = {}) => ({ s, c, o });
let trim = 0;
async function scene(spec, { ms = 0, line = null, during = null, first = false } = {}) {
  await quiet();
  await p.evaluate(([spec, z]) => {
    document.getElementById("vid-card")?.remove();
    const root = document.createElement("div");
    root.id = "vid-card";
    root.style.cssText = "position:fixed;inset:0;z-index:100000;pointer-events:none;background:#000;color:#fff;overflow:hidden;font-family:Inter,system-ui,sans-serif;";
    document.documentElement.style.zoom = "1"; // the scene is drawn at 100%, the page behind comes back to its zoom afterwards
    const make = (n) => {
      const e = document.createElement("div");
      e.style.cssText = n.s;
      const o = n.o || {};
      if (typeof n.c === "string") e.textContent = n.c; else for (const c of n.c) e.append(make(c));
      if (o.img) { const i = document.createElement("img"); i.src = o.img; i.alt = ""; i.style.cssText = n.s; return i; }
      const ms = (x) => Math.round(x * 1000);
      if (o.at !== undefined) e.animate([{ opacity: 0, transform: "translateY(24px)" }, { opacity: 1, transform: "none" }], { delay: ms(o.at), duration: 350, fill: "both", easing: "ease-out" });
      if (o.stamp !== undefined) e.animate([{ opacity: 0, transform: "rotate(-7deg) scale(2.6)" }, { opacity: 1, transform: "rotate(-7deg) scale(1)" }], { delay: ms(o.stamp), duration: 220, fill: "both", easing: "cubic-bezier(.2,1.4,.4,1)" });
      if (o.out !== undefined) e.animate([{ opacity: 1 }, { opacity: 0 }], { delay: ms(o.out), duration: 250, fill: "forwards" });
      if (o.grow !== undefined) e.animate([{ transform: "scaleX(0)" }, { transform: "scaleX(1)" }], { delay: ms(o.grow), duration: 350, fill: "both", easing: "ease-out" });
      for (const [at, col] of o.color || []) e.animate([{ color: getComputedStyle(e).color }, { color: col }], { delay: ms(at), duration: 250, fill: "forwards" });
      for (const k of o.counts || []) {
        const t0 = performance.now() + k.at * 1000;
        const tick = (now) => {
          if (now < t0) return requestAnimationFrame(tick);
          const f = Math.min(1, (now - t0) / (k.dur * 1000));
          e.textContent = (k.pre || "") + Math.round(k.from + (k.to - k.from) * (1 - Math.pow(1 - f, 3)));
          if (f < 1) requestAnimationFrame(tick);
        };
        requestAnimationFrame(tick);
      }
      return e;
    };
    root.append(make(spec));
    document.body.appendChild(root);
  }, [spec, ZOOM]);
  if (first) { T0 = Date.now(); trim = (T0 - V0) / 1000 + 0.05; } // the video starts on this card; encode.sh cuts the page load before it
  const t0 = Date.now();
  const spoken = line ? say(speech(line)) : 0;
  if (during) await during();
  const left = Math.max(ms, spoken) - (Date.now() - t0);
  if (left > 0) await wait(left);
}
const clearScene = () => p.evaluate((z) => { document.getElementById("vid-card")?.remove(); document.documentElement.style.zoom = String(z); }, ZOOM);
const RED = "#FF6B5E", GREEN = "#00EC97", MUTE = "#9aa0a6";
const MONO_ = "ui-monospace,SFMono-Regular,Menlo,Consolas,monospace";
const FULL = "display:flex;flex-direction:column;align-items:center;justify-content:center;width:100vw;height:100vh;box-sizing:border-box;text-align:center;gap:28px;";
const eyebrow = (t, at = 0) => N(`font:600 30px ${MONO_};letter-spacing:4px;color:${GREEN}`, t, { at });

// Opening, timed to the hook line (23.3 s): agents earn and get bought (0-9 s), two problems (9-16 s),
// then Polyglot's real agentexit6 numbers: 501 paid in, 140 of it from its owner, 361 counts (web/data/diligence.json).
const LAYER = "position:absolute;inset:0;" + FULL;
const openingScene = N(FULL + "background:#050505;position:relative", [
  N(LAYER, [
    N(`font:800 120px/1.05 Inter,system-ui,sans-serif;letter-spacing:-4px;color:#fff`, "Agents earn real money.", { at: 0.2, out: 9.0 }),
    N(`font:600 56px Inter,system-ui,sans-serif;color:${MUTE}`, "So people want to buy them, like small online businesses.", { at: 4.0, out: 9.0 }),
  ]),
  N(LAYER, [
    N(`font:800 120px/1.05 Inter,system-ui,sans-serif;letter-spacing:-4px;color:#fff`, "But a buyer faces two problems.", { at: 9.3, out: 15.7 }),
    N(`font:700 72px Inter,system-ui,sans-serif;color:${RED}`, "1. Revenue can be faked.", { at: 13.7, out: 15.7 }),
  ]),
  N(LAYER, [
    eyebrow("POLYGLOT · RECORDED ON NEAR TESTNET", 16.0),
    N("position:relative;display:flex;flex-direction:column;align-items:center;gap:6px", [
      N(`font:800 340px/1 Inter,system-ui,sans-serif;letter-spacing:-12px;color:#fff;min-width:900px`, "$0",
        { at: 16.0, counts: [{ from: 0, to: 501, at: 16.1, dur: 1.2, pre: "$" }, { from: 501, to: 361, at: 22.0, dur: 1.1, pre: "$" }], color: [[22.0, GREEN]] }),
      N(`position:absolute;left:0;right:0;top:170px;height:14px;background:${RED};border-radius:7px;transform-origin:left`, [], { grow: 20.0, out: 22.0 }),
      N(`font:600 44px Inter,system-ui,sans-serif;color:${MUTE}`, "paid in", { at: 16.6, out: 22.0 }),
      N(`position:absolute;left:0;right:0;top:346px;text-align:center;font:600 44px Inter,system-ui,sans-serif;color:${GREEN}`, "from real customers", { at: 22.1 }),
    ]),
    N(`font:700 54px Inter,system-ui,sans-serif;color:${RED};border:4px solid ${RED};border-radius:20px;padding:14px 40px`, "$140 paid by its own owner", { at: 20.2 }),
    N(`font:700 54px Inter,system-ui,sans-serif;color:${GREEN};border:4px solid ${GREEN};border-radius:20px;padding:14px 40px`, "only $361 counts", { at: 22.3 }),
  ]),
]);
// A 4 s card: the problem in four words.
const problemScene = N(FULL + "background:#000", [
  N(`font:800 190px/1 Inter,system-ui,sans-serif;letter-spacing:-6px;color:#fff`, "Fake revenue.", { at: 0.1 }),
  N(`font:800 190px/1 Inter,system-ui,sans-serif;letter-spacing:-6px;color:${RED}`, "Hidden keys.", { at: 1.1 }),
  N(`font:500 48px Inter,system-ui,sans-serif;color:${MUTE};margin-top:24px`, "Buying an agent today is blind trust.", { at: 2.1 }),
]);
// The climax, modelled on the deck's "how" slide: three red rows, each stamped REFUSED in time with the voice, then the struck-out key.
const row = (label, err, at, stampAt) => N(`display:flex;align-items:center;justify-content:space-between;gap:40px;width:1500px;padding:26px 48px;background:#1b0b0a;border:4px solid ${RED};border-radius:24px;position:relative`, [
  N(`font:700 58px Inter,system-ui,sans-serif;color:#fff;text-align:left`, label),
  N(`font:600 46px ${MONO_};color:${RED}`, err),
  N(`font:900 60px Inter,system-ui,sans-serif;letter-spacing:4px;color:${RED};border:6px solid ${RED};border-radius:14px;padding:6px 26px;background:#000`, "REFUSED", { stamp: stampAt }),
], { at });
const keyIcon = (col) => N("display:flex;align-items:center", [
  N(`width:44px;height:44px;border-radius:50%;border:8px solid ${col};box-sizing:border-box`),
  N(`width:70px;height:8px;background:${col}`), N(`width:8px;height:22px;background:${col};margin:22px 0 0 -26px`),
]);
const climaxScene = N(FULL + "background:#000;gap:26px", [
  eyebrow("AFTER THE SALE · ALICE TRIES EVERY WAY BACK IN"),
  row("Withdraw the treasury", "NotOwner", 2.3, 4.0),
  row("Add her own key", "NotOwner", 4.5, 6.2),
  row("Use her old key", "AccessKeyNotFound", 6.7, 8.7),
  N("display:flex;align-items:center;gap:90px;margin-top:10px", [
    N("display:flex;align-items:center;gap:20px;position:relative", [keyIcon(RED), N(`font:700 50px Inter,system-ui,sans-serif;color:${RED}`, "Alice's key"),
      N(`position:absolute;left:-14px;right:-14px;top:50%;height:8px;background:#fff;transform-origin:left`, [], { grow: 9.4 })], { at: 9.0 }),
    N("display:flex;align-items:center;gap:20px", [keyIcon(GREEN), N(`font:700 50px Inter,system-ui,sans-serif;color:${GREEN}`, "Bob's key")], { at: 9.0 }),
  ]),
  N(`font:800 76px Inter,system-ui,sans-serif;color:#fff`, "Her key is deleted, so she cannot get back in.", { at: 9.9 }),
]);
// The end card: the name first, then the one command; the page address is small.
const endScene = N(FULL + "background:#000;gap:34px", [
  N("width:380px;height:auto;border-radius:24px", [], { img: "media/logo.png" }),
  N(`font:800 120px/1 Inter,system-ui,sans-serif;letter-spacing:-4px;color:#fff`, "Agent Exit", { at: 0.2 }),
  N(`font:600 52px Inter,system-ui,sans-serif;color:${MUTE}`, "Buy agents by what they really earn.", { at: 0.9 }),
  N(`display:flex;font:700 68px ${MONO_};color:#fff;border:4px solid ${GREEN};border-radius:20px;padding:20px 52px;white-space:pre`, [N("", "make judge \u2192 "), N(`color:${GREEN}`, "9/9")], { at: 1.6 }),
  N(`font:500 34px Inter,system-ui,sans-serif;color:${MUTE}`, "Live on NEAR testnet. Test money only: test dollars (xUSD-FAKE).", { at: 2.4 }),
  N(`font:500 26px ${MONO_};color:#6b7075;margin-top:10px`, "near-transparent-agent-market-web.vercel.app", { at: 3.0 }),
]);
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

// Shot 0: open on motion (Polyglot 501 -> 361), then a 4 s problem card. The page is already loaded behind.
await home();
await scene(openingScene, { ms: 25000, line: LINES.hook, first: true });
await scene(problemScene, { ms: 4000, line: LINES.problem });
await clearScene();

// Shot 1: the market page, 2 s of hero, then the verdict cards. Alice and Polyglot come in with the voice.
await caption("Agent Exit, on NEAR: revenue counted by who paid; one transaction deletes the seller's keys", 0, LINES.market, true);
await wait(1800);
await show("#market", "start");
await p.evaluate(() => window.scrollBy(0, -30));
await caption("Agent Exit, on NEAR: revenue counted by who paid; one transaction deletes the seller's keys", 800);

// Shot 2: the verdicts a buyer sees before paying.
await caption("", 0);
await p.click('#filters button[data-f="flag"]');
await openAgent("cheat", 0);
await glow(".mk-panel .mk-bad");
const refusal = rec.steps.cheat.txs.find((t) => t.method === "list_for_sale" && t.ok === false);
await caption(`QuickQuote: on testnet, list_for_sale refused NotBornClean (tx ${refusal.tx.slice(0, 8)}…) and the buy was refunded`, 800, LINES.cheat);
await closeAgent();
await p.click('#filters button[data-f="sold"]');
await openAgent("translator", 0);
await glow(".mk-panel .mk-sold");
await caption("Polyglot: sold in one transaction. Its 140 of self-payments never counted. Alice's six tries all failed.", 800, LINES.sold);
await closeAgent();

// Shot 3: check it, on the proof page: Run and verify ticks through the recorded testnet transactions.
await caption("", 0);
await p.goto(URL_ + "proof.html");
await p.waitForSelector('#scenarios[data-ready="1"]');
await zoom(1.5);
await wait(600);
await p.evaluate(() => { document.querySelector('[data-scenario="sale"]').open = true; }); // the hash used to open it
await show('[data-scenario="sale"]', "start");
await p.evaluate(() => window.scrollBy(0, -120));
await caption("Recorded testnet answers, replayed. The live page re-checks them.", 0, LINES.evidence);
await wait(1200);
// The replay runs unseen (its half-done counts such as "4 of 12" would read as failures); the shot starts once it has finished.
const hideRun = (v) => p.evaluate((v) => {
  window.__hide ||= new CSSStyleSheet(); // a constructed sheet: the page's CSP blocks inline styles, and the run builds its list while it runs
  window.__hide.replaceSync(v === "hidden" ? '[data-scenario="sale"] .run-summary, [data-scenario="sale"] .stages { visibility: hidden !important }' : "");
  document.adoptedStyleSheets = [window.__hide];
}, v);
await hideRun("hidden");
await p.locator('[data-run="sale"]').click();
await p.waitForFunction(() => /Run again/.test(document.querySelector('[data-run="sale"]').textContent));
await p.waitForFunction(() => /12 of 12/.test(document.querySelector('[data-scenario="sale"]').textContent), null, { timeout: 30000 });
await hideRun("visible");
await wait(300);
// All 12 stages are checked: show the list, where every refused attack carries a red "refused, confirmed on chain" badge.
await show('[data-scenario="sale"] .stages', "start");
await p.evaluate(() => window.scrollBy(0, -150));
await wait(1400);

// Shot 4: invest, and AI buyers (back on the market page). The MCP scene is 4 s.
await home();
await p.click('#filters button[data-f="all"]');
await openAgent("translator3", 0);
await p.fill("#mk-account", "judge.testnet");
await glow("#mk-invest");
await caption("Open now: invest 2.5% of Lingo Live's earnings for 25 test dollars", 800, LINES.invest);
await closeAgent();
await openAgent("translate-pro", 0);
await glow(".mk-panel pre.mcp");
await caption("Agents can buy agents over MCP: scan first, spending limit enforced", 1, LINES.mcp); // hold the MCP panel for the whole line
await glow(null);
await closeAgent();

// Shot 5: the playground, the page zoomed so the changing panel reads at 1080p.
await zoom(1.5);
await show("#missions", "start");
await p.evaluate(() => window.scrollBy(0, -110));
await p.locator('#missions button[data-mission="invest"]').click();
await wait(300);
await caption("The same contract code, run in your browser: Alice sells 20% of her agent's earnings to Dave", 0, LINES.playInvest);
await act("offer_share", 900);
await act("buy_share", 1000);
await caption("Each payment pays Dave first, even one Alice makes to herself", 0, LINES.playPay);
await act("pay_customer_inv", 1000);
await act("pay_self_inv", 1200, "feed");

await quiet();
await show("#missions", "start");
await p.evaluate(() => window.scrollBy(0, -110));
await p.locator('#missions button[data-mission="honest"]').click();
await wait(300);
await caption("One transaction: Bob pays, Alice's keys are deleted", 0, LINES.playBuy);
await act("list", 600);
await act("scan", 600);
await act("buy", 1200);
// The climax: a full-screen card; Alice's three real attempts run in the playground behind it.
await caption("", 0);
await scene(climaxScene, { ms: 10900, line: LINES.breakin, during: async () => {
  await act("withdraw", 300, "feed");
  await act("set_key", 300, "feed");
  await act("old_key", 300, "feed");
} });
await clearScene();

// Shot 6: the end card.
await caption("", 0);
await scene(endScene, { ms: 5300, line: LINES.close });

await quiet();
await ctx.close();
await browser.close();
writeFileSync(new URL("../../out/video/trim.txt", import.meta.url).pathname, trim.toFixed(2) + "\n");
if (manifest) writeFileSync(new URL("../../out/video/voice-track.json", import.meta.url).pathname, JSON.stringify(track, null, 2) + "\n");
console.log(`video: raw recording in ${OUT}`);
