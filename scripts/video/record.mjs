#!/usr/bin/env node
// Records the demo video (about 2 minutes) from the page, offline: the recorded
// testnet data (web/data) and the playground (the real contract core in
// wasm). No network, no wallet. Captions are drawn on the page.
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

const W = 1280, H = 720;
const browser = await chromium.launch();
const ctx = await browser.newContext({ viewport: { width: W, height: H }, recordVideo: { dir: OUT, size: { width: W, height: H } } });
const p = await ctx.newPage();
const wait = (ms) => p.waitForTimeout(ms);
// Narration (optional): out/video/voice/manifest.json from voice.mjs. Each line
// starts when its caption shows and the next caption waits for it to end; the
// start times go to out/video/voice-track.json for encode.sh to mix in.
const T0 = Date.now();
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
      c.style.cssText = "position:fixed;inset:0 0 auto 0;margin:0;border:0;width:auto;z-index:99999;background:rgba(10,20,16,.92);color:#fff;font:600 26px/1.3 Inter,system-ui,sans-serif;padding:16px 40px;text-align:center;box-shadow:0 4px 16px rgba(0,0,0,.25)";
      c.setAttribute("popover", "manual"); // the top layer, so an open agent panel's backdrop does not dim it
      document.body.appendChild(c);
    }
    // An open agent panel sits in the top layer: keep the caption beside it, not under it.
    const d = document.querySelector("dialog[open]");
    c.style.right = d ? `${d.getBoundingClientRect().width}px` : "0";
    c.style.fontSize = big ? "34px" : "26px";
    c.textContent = t;
    c.style.display = t ? "block" : "none";
    if (c.matches(":popover-open")) c.hidePopover();
    if (t) c.showPopover();
  }, [text, big]);
  const spoken = line ? say(speech(line)) : 0;
  if (ms) await wait(Math.max(ms, spoken));
}
async function card(lines, ms, line = null) {
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
      if (l.startsWith("img:")) { const im = document.createElement("img"); im.src = l.slice(4); im.alt = ""; im.style.cssText = "width:560px;border-radius:24px"; return im; }
      const d = document.createElement("div"); d.textContent = l; if (i === 0) d.style.cssText = "font-size:56px;font-weight:800"; else if (l.startsWith("http")) d.style.cssText = "color:#3ddc97;font-size:26px"; return d;
    }));
    c.style.display = lines.length ? "flex" : "none";
  }, lines);
  const spoken = line ? say(speech(line)) : 0;
  if (ms) await wait(Math.max(ms, spoken));
}
async function scrollTo(sel, offset = -110) {
  await p.evaluate(([s, o]) => { const e = document.querySelector(s); window.scrollTo({ top: e.getBoundingClientRect().top + window.scrollY + o, behavior: "smooth" }); }, [sel, offset]);
  await wait(600);
}
// Press a mission step, then show its result (the feed, newest first).
async function act(k, ms = 2200) {
  await scrollTo("#mission-steps", -110);
  await p.locator(`#mission-steps button[data-action="${k}"]`).click();
  await wait(500);
  await scrollTo("#pg-feed", -120);
  await wait(ms);
}
async function mark(sel, on = true) {
  await p.evaluate(([s, on]) => document.querySelectorAll(".dd-card").forEach((c) => {
    c.style.outline = on && c.matches(s) ? "4px solid #f5b400" : ""; c.style.outlineOffset = "3px";
  }), [sel, on]);
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
await p.goto(URL_);
await p.waitForSelector("#market[data-ready]");
await p.waitForSelector('#play[data-ready="1"]');
const openAgent = async (name, ms) => { await p.click(`.mk-deed[data-agent="${name}"]`); await wait(400 + ms); };
const closeAgent = async () => { await p.keyboard.press("Escape"); await wait(500); };

// Shot 1: the hook and the problem.
await card(["img:media/logo.png"], 1500, LINES.hook);
await card(["Fake revenue.", "Hidden keys.", "Today, buying an AI agent is blind trust."], 1500, LINES.problem);
await card([], 0);
await caption("Every agent carries a verdict, counted from who actually paid", 1200, LINES.market);

// Shot 2: the verdicts a buyer sees before paying.
await p.click('#filters button[data-f="flag"]');
await openAgent("pump-bot", 0);
await glow(".mk-panel .kpis");
await caption("TrendPulse (ill-intended test): 400 of 410 is self-paid", 1200, LINES.pump);
await closeAgent();
await openAgent("cheat", 0);
await glow(".mk-panel .mk-bad");
await caption("QuickQuote (ill-intended test): hidden key, buy refused", 1200, LINES.cheat);
await closeAgent();
await p.click('#filters button[data-f="sold"]');
await openAgent("translator", 0);
await glow(".mk-panel .mk-sold");
await caption("Polyglot: sold in one transaction, 6 ways back in, all failed", 1200, LINES.sold);
await closeAgent();
await p.click('#filters button[data-f="all"]');
await openAgent("translator3", 0);
await p.fill("#mk-account", "judge.testnet");
await glow("#mk-invest");
await caption("Invest: 2.5% of Lingo Live's earnings for 25 test dollars", 1200, LINES.invest);
await closeAgent();
await openAgent("research-desk", 0);
await glow(".mk-panel pre.mcp");
await caption("AI agents can buy too, over MCP", 1200, LINES.mcp);
await glow(null);
await closeAgent();

// Shot 3: invest, in the real contract running in the page.
await scrollTo("#play");
await p.locator('#missions button[data-mission="invest"]').click();
await wait(300);
await caption("The real contract, in your browser: alice sells 20% to dave", 0, LINES.playInvest);
await act("offer_share", 1000);
await act("buy_share", 1400);
await caption("Each payment pays the investor; faked revenue is flagged", 0, LINES.playPay);
await act("pay_customer_inv", 1400);
await act("pay_self_inv", 1800);

// Shot 4: buy it whole, then the break-in.
await scrollTo("#play");
await p.locator('#missions button[data-mission="honest"]').click();
await wait(300);
await caption("One transaction: keys swapped, 30% waits for real customers", 0, LINES.playBuy);
await act("list", 900);
await act("scan", 1000);
await act("buy", 1800);
await caption("alice tries to get back in", 0, LINES.breakin);
await act("withdraw", 1000);
await act("set_key", 1000);
await act("old_key", 1600);

// Shot 5: the off-chain brain.
await scrollTo("#play");
await p.locator('#missions button[data-mission="brain"]').click();
await wait(300);
await caption("The prompt is sealed: only its hash goes on-chain", 0, LINES.brain);
await act("commit_config", 1000);
await act("list", 700);
await act("scan", 700);
await act("buy", 1200);
await act("pay_customer", 700);
await caption("A cheaper prompt is delivered: the hash fails, Bob is refunded", 0, LINES.brainFake);
await act("deliver_fake", 1000);
await act("open_config", 1000);
await act("dispute_config", 2400);

// Shot 6: what is new, then the end card.
await caption("", 0);
await card(["What's new", "Code, not trust, locks the seller out.", "Revenue proven by who paid · keys swapped in one transaction · price released by real customers"], 1500, LINES.whyNew);
await card(["img:media/logo.png", "Live on NEAR testnet. All money is test dollars.", "https://near-transparent-agent-market-web.vercel.app"], 2500, LINES.close);

await quiet();
await ctx.close();
await browser.close();
if (manifest) writeFileSync(new URL("../../out/video/voice-track.json", import.meta.url).pathname, JSON.stringify(track, null, 2) + "\n");
console.log(`video: raw recording in ${OUT}`);
