// Playwright smoke test for the demo page: node web/test/smoke.mjs
// Needs the playwright package with Chromium (require('playwright')).
// External requests are blocked: the offline page must work with none.
import { createRequire } from "node:module";
import assert from "node:assert/strict";
import { serve } from "../serve.mjs";

const require = createRequire(import.meta.url);
const { chromium } = require("playwright");

const server = await serve(0);
const base = `http://127.0.0.1:${server.address().port}/`;
const browser = await chromium.launch();
const problems = [];
let checks = 0;

async function page(viewport) {
  const ctx = await browser.newContext({ viewport });
  await ctx.route("**/*", (r) => (r.request().url().startsWith(base) ? r.continue() : r.abort()));
  const p = await ctx.newPage();
  p.on("console", (m) => { if (m.type() === "error") problems.push(`console: ${m.text()}`); });
  p.on("pageerror", (e) => problems.push(`pageerror: ${e.message}`));
  return p;
}

const expect = {
  "break-in": ["NotOwner", "backdoor_proven", "300.00"],
  business: ["PolicyViolation:DailyCap: 8.00 + 45.00 > daily cap 50.00", "PolicyViolation:Payee", "treasury_pay"],
  verified: ["verified (NEAR AI signature)", "120.00", "360.00"],
  cheat: ["verdict blocked", "full_access_key_present", "0.00"],
  "honest-sale": ["700.00", "settling", "bob.testnet"],
  "agent-buys-agent": ["SCAN_BLOCKED", "buy_agent", "verify_lockout"],
};

async function noHorizontalScroll(p, what) {
  const [sw, cw] = await p.evaluate(() => [document.documentElement.scrollWidth, document.documentElement.clientWidth]);
  assert.ok(sw <= cw, `${what}: horizontal scroll (${sw} > ${cw})`);
  checks++;
}

try {
  for (const vp of [{ width: 1280, height: 900 }, { width: 375, height: 740 }]) {
    const p = await page(vp);
    await p.goto(base);
    await p.waitForSelector('#step[data-step="break-in"]');
    assert.equal(await p.locator("#steps li").count(), 6);
    assert.match(await p.textContent("#mode"), /offline: recorded results/);
    const ids = Object.keys(expect);
    for (let i = 0; i < ids.length; i++) {
      const id = ids[i];
      if (i > 0) await p.click("#next");
      await p.waitForSelector(`#step[data-step="${id}"]`);
      await p.locator("#step details").evaluateAll((ds) => ds.forEach((d) => { d.open = true; }));
      const text = await p.textContent("#step");
      for (const want of expect[id]) { assert.ok(text.includes(want), `${id}: missing "${want}"`); checks++; }
      if (i > 0) assert.ok(p.url().endsWith(`#${id}`), `${id}: url hash`);
      await noHorizontalScroll(p, `${id} at ${vp.width}px`);
    }
    assert.ok(await p.isDisabled("#next"), "next disabled on the last step");
    await p.close();
  }

  // Deep link and error cards: break-in has 5 refused calls + 1 ok.
  const p = await page({ width: 375, height: 740 });
  await p.goto(`${base}#business`);
  await p.waitForSelector('#step[data-step="business"]');
  assert.ok((await p.locator('#step .call[data-outcome="error"]').count()) >= 3);
  await p.goto(`${base}#break-in`);
  await p.waitForSelector('#step[data-step="break-in"]');
  assert.equal(await p.locator('#step .call[data-outcome="error"]').count(), 5);
  checks += 2;

  // Testnet flag: labelled, offers login, never loaded unless asked.
  await p.goto(`${base}?network=testnet`);
  await p.waitForSelector("#testnet-label");
  assert.equal((await p.textContent("#testnet-label")).trim(), "testnet: coming after deploy");
  assert.ok(await p.isVisible("#login"));
  await noHorizontalScroll(p, "testnet mode at 375px");
  checks += 2;
  await p.close();

  assert.deepEqual(problems, [], "no console errors");
  console.log(`web-test: ${checks} checks passed (desktop + 375px, offline + testnet flag)`);
} finally {
  await browser.close();
  server.close();
}
