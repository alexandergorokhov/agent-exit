#!/usr/bin/env node
// Records the demo video (about 2 minutes) from the page, offline: the recorded
// testnet data (web/data) and the playground (the real contract core in
// wasm). No network, no wallet. Captions are drawn on the page.
//   node web/serve.mjs 8090 &   then   node scripts/video/record.mjs
// Writes out/video/raw/*.webm; scripts/video/encode.sh makes the mp4 + GIF.
import { createRequire } from "node:module";
import { mkdirSync, rmSync } from "node:fs";
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

async function caption(text, ms = 3000, big = false) {
  await p.evaluate(([t, big]) => {
    let c = document.getElementById("vid-cap");
    if (!c) {
      c = document.createElement("div");
      c.id = "vid-cap";
      c.style.cssText = "position:fixed;left:0;right:0;top:0;z-index:99999;background:rgba(10,20,16,.88);color:#fff;font:600 26px/1.3 Inter,system-ui,sans-serif;padding:16px 40px;text-align:center;box-shadow:0 4px 16px rgba(0,0,0,.25)";
      document.body.appendChild(c);
    }
    // An open agent panel sits in the top layer: keep the caption beside it, not under it.
    const d = document.querySelector("dialog[open]");
    c.style.right = d ? `${d.getBoundingClientRect().width}px` : "0";
    c.style.fontSize = big ? "34px" : "26px";
    c.textContent = t;
    c.style.display = t ? "block" : "none";
  }, [text, big]);
  if (ms) await wait(ms);
}
async function card(lines, ms) {
  await p.evaluate((lines) => {
    let c = document.getElementById("vid-card");
    if (!c) {
      c = document.createElement("div");
      c.id = "vid-card";
      c.style.cssText = "position:fixed;inset:0;z-index:100000;background:#0b1510;color:#fff;display:flex;flex-direction:column;align-items:center;justify-content:center;gap:18px;font:600 30px/1.35 Inter,system-ui,sans-serif;text-align:center;padding:40px";
      document.body.appendChild(c);
    }
    c.replaceChildren(...lines.map((l, i) => { const d = document.createElement("div"); d.textContent = l; if (i === 0) d.style.cssText = "font-size:56px;font-weight:800"; else if (l.startsWith("http")) d.style.cssText = "color:#3ddc97;font-size:26px"; return d; }));
    c.style.display = lines.length ? "flex" : "none";
  }, lines);
  if (ms) await wait(ms);
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

await p.goto(URL_);
await p.waitForSelector("#market[data-ready]");
await p.waitForSelector('#play[data-ready="1"]');
const openAgent = async (name, ms) => { await p.click(`.mk-deed[data-agent="${name}"]`); await wait(400 + ms); };
const closeAgent = async () => { await p.keyboard.press("Escape"); await wait(500); };

// Shot 1: the claim, then the market.
await card(["Agent Exit", "Buy AI agents by what they really earn.", "Live on NEAR testnet. All money is test dollars."], 4000);
await card([], 0);
await caption("A market of AI agents, recorded on NEAR testnet. Each one carries a verdict.", 4000);

// Shot 2: the verdicts a buyer sees before paying.
await p.click('#filters button[data-f="flag"]');
await caption("Flagged: revenue its own owner paid, or a hidden key", 2500);
await openAgent("pump-bot", 0);
await caption("pump-bot shows 410.00 of revenue. 400.00 came from its owner, so only 10.00 counts.", 5000);
await closeAgent();
await openAgent("cheat", 0);
await caption("cheat: all revenue self-paid, and a hidden key. Buying is refused before any money moves.", 5000);
await closeAgent();
await p.click('#filters button[data-f="sold"]');
await openAgent("translator", 0);
await caption("translator was sold in one transaction. The seller then tried 6 ways back in. All 6 failed on testnet.", 5500);
await closeAgent();
await p.click('#filters button[data-f="all"]');
await openAgent("translator3", 0);
await caption("translator3: every counted job signed by NEAR AI. Anyone can buy 2.5% of its earnings for 25 test dollars.", 5000);
await p.fill("#mk-account", "judge.testnet");
await caption("Type a testnet account: one signature in MyNearWallet buys the share", 3500);
await closeAgent();
await openAgent("research-desk", 0);
await caption("AI agents can buy too: one MCP call, and the server scans before it pays", 4500);
await closeAgent();

// Shot 3: invest instead of buying (business first).
await scrollTo("#play");
await p.locator('#missions button[data-mission="invest"]').click();
await wait(600);
await caption("Proven earnings unlock money. alice sells 20% of future earnings to dave", 0);
await act("offer_share", 1800);
await act("buy_share", 2600);
await caption("Each customer payment pays the investor inside the same call", 0);
await act("pay_customer_inv", 2800);
await caption("alice pays her own agent to fake revenue: flagged, not counted, and the investor still gets 20%", 0);
await act("pay_self_inv", 3800);

// Shot 4: the honest sale, in the real contract logic.
await scrollTo("#play");
await p.locator('#missions button[data-mission="honest"]').click();
await wait(600);
await caption("Or buy it whole. The real contract logic, compiled into the page: list, scan, buy", 0);
await act("list", 1800);
await act("scan", 2200);
await act("buy", 3200);
await caption("One transaction: bob pays, alice's keys are deleted, 70% to alice now less a 1% fee, 30% held for the earn-out", 0);
await wait(3000);
await caption("Now alice tries to get back in", 2000);
await act("withdraw", 2200);
await act("set_key", 2200);
await act("old_key", 2600);

// Shot 5: the off-chain brain. A swapped prompt returns the whole price.
await scrollTo("#play");
await p.locator('#missions button[data-mission="brain"]').click();
await wait(600);
await caption("The agent's value is also its prompt. alice commits to it sealed: only its hash goes on-chain", 0);
await act("commit_config", 2400);
await act("list", 1500);
await act("scan", 1800);
await act("buy", 2600);
await act("pay_customer", 1500);
await caption("alice delivers a cheaper prompt instead", 0);
await act("deliver_fake", 2400);
await act("open_config", 2400);
await caption("The contract decrypts and hashes the delivery itself: it does not match, so the whole price goes back to bob", 0);
await act("dispute_config", 4200);

// Shot 6: how a sale works, in one line each.
await scrollTo("#how");
await caption("Check, buy in one transaction, seller locked out. Every claim is checked on testnet on the For judges page.", 5000);

// End card.
await caption("", 0);
await card(["Agent Exit", "Buy AI agents by what they really earn.", "Live on NEAR testnet. All money is fake test money.", "https://near-transparent-agent-market-web.vercel.app"], 5000);

await ctx.close();
await browser.close();
console.log(`video: raw recording in ${OUT}`);
