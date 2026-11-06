#!/usr/bin/env node
// Records the 80-second demo video from the page, offline: the recorded
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
await p.waitForSelector("#proof-strip .ps-stats li");
await p.waitForSelector("#dd-cards .dd-card");

// Shot 1 (0:00-0:14): the claim, then the proof on the first screen.
await card(["Agent Exit", "Turn an AI agent into a business you can trust.", "Earnings proven on-chain. Invest in a share, or buy it whole. The seller is locked out."], 4000);
await card([], 0);
await caption("Recorded on NEAR testnet: 5 of 5 customer payments signed by NEAR AI and checked by the contract", 3800);
await caption("An investor bought 20% of an agent's earnings and got exactly 20%", 3200);
await caption("After a one-transaction sale, the old seller tried 6 ways back in on testnet. All refused.", 3500);

// Shot 2 (0:14-0:34): what a buyer sees before paying.
await scrollTo("#diligence");
await caption("Before paying, a buyer sees revenue split by who paid", 3500);
await mark('[data-agent="cheat"]');
await caption("cheat: all 500.00 was paid by the owner and the owner's sub-accounts, plus a hidden full-access key. Do not buy.", 5500);
await mark('[data-agent="translator"],[data-agent="translator3"]');
await caption("translator: 72% from outside customers, clean keys. translator3: every job signed by NEAR AI.", 5500);

await mark("", false);
// Shot 3 (0:34-0:58): the honest sale, in the real contract logic.
await scrollTo("#play");
await p.locator('#missions button[data-mission="honest"]').click();
await wait(600);
await caption("The real contract logic, compiled into the page: list, scan, buy", 0);
await act("list", 1800);
await act("scan", 2200);
await act("buy", 3200);
await caption("One transaction: bob pays, alice's keys are deleted, 70% to alice now less a 1% fee, 30% held for the earn-out", 0);
await wait(3000);
await caption("Now alice tries to get back in", 2000);
await act("withdraw", 2200);
await act("set_key", 2200);
await act("old_key", 2600);

// Shot 4 (0:58-1:18): invest instead of buying.
await scrollTo("#play");
await p.locator('#missions button[data-mission="invest"]').click();
await wait(600);
await caption("Or invest: alice sells 20% of future earnings", 0);
await act("offer_share", 1800);
await act("buy_share", 2600);
await caption("Each customer payment pays the investor inside the same call", 0);
await act("pay_customer_inv", 2800);
await caption("alice pays her own agent to fake revenue: flagged, not counted, and the investor still gets 20%", 0);
await act("pay_self_inv", 3800);

// End card.
await caption("", 0);
await card(["Agent Exit", "Proven earnings. Shares. A clean exit.", "Live on NEAR testnet. All money is fake test money.", "https://near-transparent-agent-market-web.vercel.app"], 5000);

await ctx.close();
await browser.close();
console.log(`video: raw recording in ${OUT}`);
