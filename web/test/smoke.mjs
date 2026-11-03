// Playwright smoke test for the demo page: node web/test/smoke.mjs
// Needs the playwright package with Chromium (require('playwright')).
// External requests are blocked: the offline page must work with none.
import { createRequire } from "node:module";
import assert from "node:assert/strict";
import { createHash } from "node:crypto";
import { readFileSync } from "node:fs";
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
  "agent-buys-agent": ["scripted replay", "SCAN_BLOCKED", "buy_agent", "verify_lockout", "Real AI buyer transcript (recorded)", "claude-haiku-4-5", "verdict clean", "seller locked out: true"],
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

  // P1-P3: the playground runs the real contract core (wasm) in the page; each mission ends in its lesson.
  {
    const [sum] = readFileSync(new URL("../playground.wasm.sha256", import.meta.url), "utf8").trim().split(/\s+/);
    assert.equal(createHash("sha256").update(readFileSync(new URL("../playground.wasm", import.meta.url))).digest("hex"), sum, "P0: playground.wasm matches its pinned SHA-256 (make playground-wasm)");
    const want = {
      fake: ["FLAGGED: the owner paid their own agent", "FLAGGED: paid from the owner's sub-account", "Verified (NEAR AI signature)"],
      backdoor: ["scan -> blocked (full_access_key_present)", "SCAN_BLOCKED", "FROZEN", "300.00 of escrow goes back to the buyer"],
      honest: ["scan -> clean", "700.00 paid to alice now, 300.00 held as earn-out", "withdraw -> NotOwner", "AccessKeyNotFound", "earn-out closed"],
    };
    for (const vp of [{ width: 1280, height: 900 }, { width: 375, height: 740 }]) {
      const q = await page(vp);
      await q.goto(base);
      await q.waitForSelector('#play[data-ready="1"]');
      for (const [id, texts] of Object.entries(want)) {
        await q.click(`[data-mission="${id}"]`);
        while (await q.locator("#mission-steps .act.next").count()) await q.click("#mission-steps .act.next");
        assert.ok(await q.isVisible("#mission-lesson"), `P: ${id} lesson shown`);
        const feed = await q.textContent("#pg-feed");
        for (const t of texts) { assert.ok(feed.includes(t), `P: ${id}: missing "${t}"`); checks++; }
      }
      assert.match(await q.textContent("#pg-state"), /Owner\s*bob\.testnet/, "P3: bob owns the agent after the honest sale");
      // "How this works for real": the honest buy links its code and the same call on testnet.
      const buy = q.locator('#pg-feed .feed-item[data-action="buy"]').first();
      await buy.locator("details.real summary").click();
      assert.ok((await buy.locator('a[href*="contracts/agent-core/src/lib.rs#L"]').count()) === 1, "P: buy links the Rust function");
      assert.ok((await buy.locator('a[href^="https://testnet.nearblocks.io/txns/"]').count()) === 1, "P: buy links the same call on testnet");
      checks += 2;
      await noHorizontalScroll(q, `playground at ${vp.width}px`);
      await q.close();
    }
    checks += 1;
  }

  // P4: every code link in the playground points at the function it names.
  {
    const src = readFileSync(new URL("../playground.js", import.meta.url), "utf8");
    const re = /code: \[(?:"([^"]+)"|CORE), (\d+), "([^"]+)"\]/g;
    let m, n = 0;
    while ((m = re.exec(src))) {
      const line = readFileSync(new URL(`../../${m[1] || "contracts/agent-core/src/lib.rs"}`, import.meta.url), "utf8").split("\n")[Number(m[2]) - 1] || "";
      assert.ok(line.includes(m[3]), `P4: ${m[1] || "core"}:${m[2]} is not "${m[3]}": ${line.trim()}`);
      n++;
    }
    assert.ok(n >= 10, `P4: ${n} code links checked`);
    checks += n;
  }

  // S23: copy matches the evidence.
  {
    const q = await page({ width: 1280, height: 900 });
    await q.goto(`${base}#break-in`);
    await q.waitForSelector('#step[data-step="break-in"]');
    const planted = q.locator('#step .call[data-outcome="frozen"]');
    assert.equal(await planted.count(), 1, "S23: the planted-key heartbeat is one FROZEN card");
    assert.match(await planted.textContent(), /FROZEN/);
    assert.equal(await q.locator('#step .call[data-outcome="ok"]').count(), 0, "S23: no green OK on the break-in step");
    const tn = await q.locator('#step .testnet a[href^="https://testnet.nearblocks.io/txns/"]').count();
    assert.ok(tn >= 6, `the break-in step links the testnet run (${tn} links)`);
    await q.goto(`${base}#verified`);
    await q.waitForSelector('#step[data-step="verified"]');
    const reused = q.locator('#step .call[data-outcome="unverified"]');
    assert.equal(await reused.count(), 1, "S23: the reused signature is labelled");
    assert.match((await reused.textContent()).toLowerCase(), /counted, not verified/);
    await q.goto(`${base}#cheat`);
    await q.waitForSelector('#step[data-step="cheat"]');
    const cheat = await q.textContent("#step");
    assert.ok(!cheat.includes("two RPC providers") || cheat.includes("not wired yet"), "S23: offline step 4 must not claim two RPC providers");
    assert.ok(cheat.includes("recorded key list"), "S23: step 4 says the key list is recorded");
    await q.goto(`${base}#agent-buys-agent`);
    await q.waitForSelector('#step[data-step="agent-buys-agent"]');
    assert.equal(await q.locator('#step [data-slot="ai-buyer-transcript"]').count(), 1, "S23: slot for the AI buyer transcript");
    assert.ok(!(await q.textContent("#step")).includes("An agent buys an agent"));
    // counts in the footer come from the golden attack report, not typed by hand
    const golden = JSON.parse(readFileSync(new URL("../../tests/golden/attack.json", import.meta.url), "utf8"));
    const foot = await q.textContent("footer");
    assert.ok(foot.includes(`${golden.families.length} attack families`), "S23: family count is current");
    assert.ok(foot.includes(`${golden.summary.families} counted`), "S23: counted-family count is current");
    assert.ok(foot.includes("81 MCP tests"), "S23: MCP test count is current");
    assert.ok(foot.includes("regression suite, not independent evidence"));
    checks += 8;
    await q.close();
  }

  // S24: strict CSP meta, no remote script, vendored connector pinned by hash.
  {
    const html = readFileSync(new URL("../index.html", import.meta.url), "utf8");
    const csp = /http-equiv="Content-Security-Policy" content="([^"]+)"/.exec(html);
    assert.ok(csp, "S24: CSP meta tag present");
    for (const want of ["default-src 'none'", "script-src 'self'", "style-src 'self'", "connect-src 'self'", "base-uri 'none'", "form-action 'none'"]) assert.ok(csp[1].includes(want), `S24: CSP lacks ${want}`);
    assert.ok(!/unsafe-|https?:|\*/.test(csp[1].replace("'wasm-unsafe-eval'", "")), "S24: CSP has no unsafe-* (except wasm compile), remote host or wildcard");
    // L6: the server and the static-host files send the meta CSP plus frame-ancestors, nosniff and a referrer policy.
    const want = `${csp[1]}; frame-ancestors 'none'`;
    for (const path of ["", "style.css", "missing-file"]) {
      const r = await fetch(base + path);
      assert.equal(r.headers.get("content-security-policy"), want, `L6: CSP header on /${path}`);
      assert.equal(r.headers.get("x-content-type-options"), "nosniff", `L6: nosniff on /${path}`);
      assert.equal(r.headers.get("referrer-policy"), "no-referrer", `L6: referrer policy on /${path}`);
    }
    const hdrs = readFileSync(new URL("../_headers", import.meta.url), "utf8");
    assert.ok(hdrs.includes(`Content-Security-Policy: ${want}`) && hdrs.includes("X-Content-Type-Options: nosniff") && hdrs.includes("Referrer-Policy: no-referrer"), "L6: _headers matches");
    const vj = JSON.parse(readFileSync(new URL("../vercel.json", import.meta.url), "utf8")).headers[0].headers;
    const val = (k) => vj.find((h) => h.key === k)?.value;
    assert.equal(val("Content-Security-Policy"), want, "L6: vercel.json CSP matches");
    assert.equal(val("X-Content-Type-Options"), "nosniff");
    assert.equal(val("Referrer-Policy"), "no-referrer");
    const [sum, file] = readFileSync(new URL("../vendor/SHA256SUMS", import.meta.url), "utf8").trim().split(/\s+/);
    assert.equal(createHash("sha256").update(readFileSync(new URL(`../vendor/${file}`, import.meta.url))).digest("hex"), sum, "S24: vendored connector matches its pinned SHA-256");
    assert.ok(readFileSync(new URL("../app.js", import.meta.url), "utf8").includes(`./vendor/${file}`), "S24: app.js loads the vendored file");
    // Under the CSP a page must run with zero CSP violations (console errors are collected in `problems`).
    const q = await page({ width: 1280, height: 900 });
    const seen = [];
    q.on("console", (m) => { if (/Content Security Policy/i.test(m.text())) seen.push(m.text()); });
    await q.goto(`${base}?network=testnet`);
    await q.waitForSelector("#testnet-label");
    assert.deepEqual(seen, [], "S24: no CSP violations while rendering");
    // An injected inline script must not run.
    const before = problems.length;
    await q.evaluate(() => { const s = document.createElement("script"); s.textContent = "window.__pwn = 1"; document.body.append(s); });
    assert.equal(await q.evaluate(() => window.__pwn), undefined, "S24: inline script blocked by CSP");
    await q.waitForTimeout(200);
    for (let i = problems.length - 1; i >= before; i--) if (/Content Security Policy/i.test(problems[i])) problems.splice(i, 1); // the one expected violation
    checks += 7;
    await q.close();
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
  assert.equal((await p.textContent("#testnet-label")).trim(), "testnet: run of 2026-11-03 (recorded; \"Read live\" checks the chain now)");
  assert.equal(await p.locator("#login").count(), 0, "S27: no wallet login button until testnet mode is live");
  await noHorizontalScroll(p, "testnet mode at 375px");
  checks += 2;
  await p.close();

  // L7: "Read live" goes only to the same-origin relay, which refuses anything but view queries.
  {
    const { refuse } = await import("../rpc-allow.mjs");
    const q = (params) => ({ jsonrpc: "2.0", id: 1, method: "query", params: { finality: "final", ...params } });
    assert.equal(refuse(q({ request_type: "view_account", account_id: "translator.agentexit.testnet" })), null);
    assert.equal(refuse(q({ request_type: "call_function", account_id: "translator.agentexit.testnet", method_name: "get_status", args_base64: "e30=" })), null);
    assert.ok(refuse({ jsonrpc: "2.0", id: 1, method: "broadcast_tx_commit", params: ["AAAA"] }), "L7: sending a tx is refused");
    assert.ok(refuse(q({ request_type: "call_function", account_id: "translator.agentexit.testnet", method_name: "withdraw", args_base64: "e30=" })), "L7: non-view method refused");
    assert.ok(refuse(q({ request_type: "view_account", account_id: "agentexit.near" })), "L7: mainnet account refused");
    assert.ok(refuse(q({ request_type: "view_state", account_id: "translator.agentexit.testnet", prefix_base64: "" })), "L7: view_state refused");
    const r = await fetch(`${base}api/rpc`, { method: "POST", body: JSON.stringify({ jsonrpc: "2.0", id: 1, method: "send_tx", params: {} }) });
    assert.equal(r.status, 400, "L7: local relay refuses a send");
    // The Vercel function itself must load as ESM (a .js file there loaded as
    // CommonJS and crashed on the hosted page) and refuse a send.
    const { default: vercelRpc } = await import("../api/rpc.mjs");
    const vr = { code: 0, setHeader() {}, status(c) { this.code = c; return this; }, json() {} };
    await vercelRpc({ method: "POST", body: { jsonrpc: "2.0", id: 1, method: "broadcast_tx_commit", params: ["AAAA"] } }, vr);
    assert.equal(vr.code, 400, "L7: Vercel function loads and refuses a send");
    checks += 8;

    // Mocked chain: the browser renders what the relay returns.
    const ctx = await browser.newContext({ viewport: { width: 375, height: 740 } });
    const seenHosts = new Set();
    await ctx.route("**/*", async (route) => {
      const u = route.request().url();
      seenHosts.add(new URL(u).host);
      if (!u.startsWith(base)) return route.abort();
      if (!u.startsWith(`${base}api/rpc`)) return route.continue();
      const b = JSON.parse(route.request().postData());
      const acc = b.params.account_id;
      const cheat = acc.startsWith("cheat.");
      const result = b.params.request_type === "view_account" ? { code_hash: "7K1Q5DWYuTKwbtUU8uvzrW4kyyFXG7ZVxvJDX7vAEWHq" }
        : b.params.request_type === "view_access_key_list" ? { keys: [{ access_key: { permission: "FullAccess" } }, { access_key: { permission: { FunctionCall: {} } } }].slice(cheat ? 0 : 1) }
        : { result: [...new TextEncoder().encode(JSON.stringify({ owner: cheat ? "carol.agentexit3.testnet" : "bob.agentexit3.testnet", status: "Owned", treasury: "300.00" }))] };
      return route.fulfill({ contentType: "application/json", body: JSON.stringify({ jsonrpc: "2.0", id: b.id, result }) });
    });
    const lp = await ctx.newPage();
    lp.on("pageerror", (e) => problems.push(`pageerror: ${e.message}`));
    await lp.goto(`${base}?network=testnet`);
    await lp.click("#read-live");
    await lp.waitForSelector("#live h3");
    const live = await lp.textContent("#live");
    for (const want of ["translator.agentexit3.testnet", "owner bob.agentexit3.testnet", "1 access keys, 0 full-access", "2 access keys, 1 full-access", "same as the recorded run"]) { assert.ok(live.includes(want), `L7: live box lacks "${want}"`); checks++; }
    await noHorizontalScroll(lp, "live box at 375px");
    assert.deepEqual([...seenHosts], [new URL(base).host], "L7: the page contacts only its own origin");
    checks++;
    await ctx.close();
  }

  assert.deepEqual(problems, [], "no console errors");
  console.log(`web-test: ${checks} checks passed (desktop + 375px, offline + testnet flag)`);
} finally {
  await browser.close();
  server.close();
}
