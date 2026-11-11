// Playwright smoke test for the demo page: node web/test/smoke.mjs
// Needs the playwright package with Chromium (require('playwright')).
// External requests are blocked: the offline page must work with none.
import { createRequire } from "node:module";
import assert from "node:assert/strict";
import { createHash } from "node:crypto";
import { readFileSync } from "node:fs";
import { serve } from "../serve.mjs";
import { WALLET_SIGN, encodeTx, investTx, base64 } from "../wallet-link.mjs";
import { fmt, investorPayout, waterfall, earlyNote, investorList } from "../ledger.mjs";
import { execFileSync } from "node:child_process";
import { readdirSync, statSync } from "node:fs";

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
    // The walkthrough is folded under "Offline replay"; #replay opens it on step 1.
    await p.goto(`${base}#replay`);
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
      fake: ["FLAGGED: the owner paid their own agent", "FLAGGED: paid from the owner's sub-account", "Verified (NEAR AI signature; signed with a test key here"],
      backdoor: ["scan -> blocked (full_access_key_present)", "SCAN_BLOCKED", "hidden key is still on the account", "Only the scan protects the buyer"],
      revenue: ["0.00 paid to alice now: all 1,000.00 is held", "paid 200.00 and got 100.00 back: faking revenue cost 100.00", "earn-out closed"],
      brain: ["commit_config(sha256", "DIFFERENT prompt", "does NOT match the commitment", "1,000.00 back to bob (the whole price"],
      honest: ["scan -> clean", "690.00 paid to alice now (after the 1% protocol fee, 10.00), 300.00 held as earn-out", "withdraw -> NotOwner", "AccessKeyNotFound", "earn-out closed"],
    };
    for (const vp of [{ width: 1280, height: 900 }, { width: 375, height: 740 }]) {
      const q = await page(vp);
      await q.goto(base);
      await q.waitForSelector('#play[data-ready="1"]');
      // G1 (loop 45): a disabled step says why.
      for (const id of Object.keys(want)) {
        await q.click(`[data-mission="${id}"]`);
        const later = q.locator("#mission-steps .act.later");
        const n = await later.count();
        assert.ok(n > 0, `G1: ${id} has later steps`);
        for (let i = 0; i < n; i++) {
          assert.ok(await later.nth(i).isDisabled(), `G1: ${id} later step disabled`);
          assert.match(await later.nth(i).textContent(), /Do step \d+ first/, `G1: ${id}: a disabled step says why`);
        }
        checks++;
      }
      for (const [id, texts] of Object.entries(want)) {
        await q.click(`[data-mission="${id}"]`);
        while (await q.locator("#mission-steps .act.next").count()) await q.click("#mission-steps .act.next");
        assert.ok(await q.isVisible("#mission-lesson"), `P: ${id} lesson shown`);
        const feed = await q.textContent("#pg-feed");
        for (const t of texts) { assert.ok(feed.includes(t), `P: ${id}: missing "${t}"`); checks++; }
      }
      assert.match(await q.textContent("#pg-state"), /Owner\s*bob\.testnet/, "P3: bob owns the agent after the honest sale");
      // P5: mission 4, invest: dave is paid inside every payment, even alice's fake one, and gets his escrow back.
      {
        const r = await q.locator('#pg-feed .feed-item[data-action="buy"]').first();
        await r.locator("details.real summary").click();
        await q.click('[data-mission="invest"]');
        while (await q.locator("#mission-steps .act.next").count()) await q.click("#mission-steps .act.next");
        assert.ok(await q.isVisible("#mission-lesson"), "P5: invest lesson shown");
        const feed = await q.textContent("#pg-feed");
        for (const t of ["250.00 held in escrow", "5.00 to investors", "FLAGGED", "40.00 to investors", "45.00 sent to dave", "245.00 of unreleased escrow back to dave"]) {
          assert.ok(feed.includes(t), `P5: missing "${t}"`); checks++;
        }
        assert.equal((await q.textContent(".dave-claim")).trim(), "245.00", "P5: dave can claim his escrow back"); checks++;
        await q.click('[data-mission="honest"]');
        while (await q.locator("#mission-steps .act.next").count()) await q.click("#mission-steps .act.next");
      }
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
    // counts in "Honest limits" come from the golden attack report, not typed by hand
    const golden = JSON.parse(readFileSync(new URL("../../tests/golden/attack.json", import.meta.url), "utf8"));
    const foot = await q.textContent("#for-judges");
    assert.ok(foot.includes(`${golden.families.length} attack families`), "S23: family count is current");
    assert.ok(foot.includes(`${golden.summary.families} counted`), "S23: counted-family count is current");
    assert.ok(/\d+ Rust tests and \d+ MCP tests/.test(foot), "S23: test counts shown (B4 checks them)");
    assert.ok(foot.includes("regression suite, not independent evidence"));
    checks += 8;
    await q.close();
  }

  // W10: wallet-link's borsh matches @near-js/transactions byte for byte (vector made once with @near-js/transactions 1.3.3).
  {
    const offer = { xusd: "xusd.agentexit4.testnet", agent: "translator3.agentexit4.testnet", amount: "10000000", msg: "{\"kind\":\"buy_share\"}" };
    assert.equal(base64(encodeTx(investTx(offer, "you.testnet"))), "CwAAAHlvdS50ZXN0bmV0AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAXAAAAeHVzZC5hZ2VudGV4aXQ0LnRlc3RuZXQAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAIAAAACBgAAAGZhdWNldBUAAAB7ImFtb3VudCI6IjEwMDAwMDAwIn0A4FfrSBsAAAAAAAAAAAAAAAAAAAAAAAACEAAAAGZ0X3RyYW5zZmVyX2NhbGxlAAAAeyJyZWNlaXZlcl9pZCI6InRyYW5zbGF0b3IzLmFnZW50ZXhpdDQudGVzdG5ldCIsImFtb3VudCI6IjEwMDAwMDAwIiwibXNnIjoie1wia2luZFwiOlwiYnV5X3NoYXJlXCJ9In0AYLeYbIgAAAEAAAAAAAAAAAAAAAAAAAA=", "W10: borsh golden vector");
    assert.throws(() => encodeTx({ signerId: "x", receiverId: "a.testnet", actions: [] }), "W10: rejects bad input");
    checks++;
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
  assert.equal((await p.textContent("#testnet-label")).trim(), "testnet: agentexit5, run of 2026-11-11 (recorded; \"Read live\" checks the chain now)");
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

    // Mocked chain: the browser renders what the relay returns, for the recorded world.
    const world = JSON.parse(readFileSync(new URL("../data/testnet.json", import.meta.url), "utf8"));
    const R = world.root;
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
      const result = b.params.request_type === "view_account" ? { code_hash: world.agent_code_hash }
        : b.params.request_type === "view_access_key_list" ? { keys: [{ access_key: { permission: "FullAccess" } }, { access_key: { permission: { FunctionCall: {} } } }].slice(cheat ? 0 : 1) }
        : { result: [...new TextEncoder().encode(JSON.stringify({ owner: cheat ? `carol.${R}` : `bob.${R}`, status: "Owned", treasury: "300.00" }))] };
      return route.fulfill({ contentType: "application/json", body: JSON.stringify({ jsonrpc: "2.0", id: b.id, result }) });
    });
    const lp = await ctx.newPage();
    lp.on("pageerror", (e) => problems.push(`pageerror: ${e.message}`));
    await lp.goto(`${base}?network=testnet`);
    await lp.click("#read-live");
    await lp.waitForSelector("#live h3");
    const live = await lp.textContent("#live");
    for (const want of [`translator.${R}`, `owner bob.${R}`, "1 access keys, 0 full-access", "2 access keys, 1 full-access", "same as the recorded run"]) { assert.ok(live.includes(want), `L7: live box lacks "${want}"`); checks++; }
    await noHorizontalScroll(lp, "live box at 375px");
    assert.deepEqual([...seenHosts], [new URL(base).host], "L7: the page contacts only its own origin");
    checks++;
    await ctx.close();
  }

  // C1-C2: "Check any NEAR account" reads keys and code through the relay and
  // gives the same verdicts as the MCP tool check_account.
  {
    const world = JSON.parse(readFileSync(new URL("../data/testnet.json", import.meta.url), "utf8"));
    const wasm = Buffer.from("0061736d0100000001040160000003020100070b01077570677261646500000a040102000b", "hex") /* a module exporting one function, "upgrade" */;
    const ctx = await browser.newContext({ viewport: { width: 375, height: 740 } });
    const seen = new Set();
    await ctx.route("**/*", async (route) => {
      const u = route.request().url();
      seen.add(new URL(u).host);
      if (!u.startsWith(base)) return route.abort();
      if (!u.startsWith(`${base}api/rpc`)) return route.continue();
      const b = JSON.parse(route.request().postData());
      const acc = b.params.account_id;
      const bot = acc === "bot.testnet";
      const rt = b.params.request_type;
      const result = rt === "view_account" ? { code_hash: bot ? "Bot1111111111111111111111111111111111111111" : world.agent_code_hash }
        : rt === "view_access_key_list" ? { keys: bot ? [{ public_key: "ed25519:Full", access_key: { permission: "FullAccess" } }] : [{ public_key: "ed25519:Runner", access_key: { permission: { FunctionCall: { receiver_id: acc, method_names: ["heartbeat"], allowance: "5000" } } } }] }
          : rt === "view_code" ? { code_base64: (bot ? wasm : Buffer.from("0061736d0100000001040160000003020100070d010968656172746265617400000a040102000b", "hex") /* exports only "heartbeat" */).toString("base64") }
            : { result: [...new TextEncoder().encode(JSON.stringify({ known_keys: ["ed25519:Runner"] }))] };
      return route.fulfill({ contentType: "application/json", body: JSON.stringify({ jsonrpc: "2.0", id: b.id, result }) });
    });
    const cp = await ctx.newPage();
    cp.on("pageerror", (e) => problems.push(`pageerror: ${e.message}`));
    await cp.goto(base);
    for (const [acc, want, more] of [["bot.testnet", "backdoor_risk", "code_change_method"], [`agent.${world.root}`, "locked", "Locked"]]) {
      await cp.fill("#check-account", acc);
      await cp.click("#check-form button");
      await cp.waitForFunction((w) => document.querySelector("#check-out .badge")?.textContent === w, want);
      const t = await cp.textContent("#check-out");
      assert.ok(t.includes(more), `C1: ${acc} lacks "${more}": ${t}`);
      checks++;
    }
    await noHorizontalScroll(cp, "check box at 375px");
    assert.deepEqual([...seen], [new URL(base).host], "C2: the check box contacts only its own origin");
    checks++;
    await ctx.close();
  }

  // V1-V3: "Run and verify" asks the chain for each recorded transaction and compares; a mismatch is shown, not hidden.
  {
    const rec = JSON.parse(readFileSync(new URL("../data/testnet.json", import.meta.url), "utf8"));
    const byHash = new Map(Object.values(rec.steps).flatMap((s) => s.txs).filter((t) => t.tx).map((t) => [t.tx, t]));
    const cheatKey = /(ed25519:\w+)/.exec(rec.steps.cheat.scan.findings[0].detail)[1];
    for (const lie of [false, true]) {
      const ctx = await browser.newContext({ viewport: { width: 375, height: 740 } });
      const asked = [];
      await ctx.route("**/*", async (route) => {
        const u = route.request().url();
        if (!u.startsWith(base)) return route.abort();
        if (!u.startsWith(`${base}api/rpc`)) return route.continue();
        const b = JSON.parse(route.request().postData());
        asked.push(b.method);
        let result;
        if (b.method === "tx") {
          const t = byHash.get(b.params.tx_hash);
          const logs = t.events.map((e) => `EVENT_JSON:${JSON.stringify({ standard: "agent_exit", event: e, data: [] })}`);
          result = {
            status: t.ok ? { SuccessValue: "" } : { Failure: { ActionError: { kind: { FunctionCallError: { ExecutionError: "Smart contract panicked: NotOwner" } } } } },
            transaction: { signer_id: lie ? "mallory.testnet" : t.by, receiver_id: t.receiver, actions: [{ FunctionCall: { method_name: t.method } }] },
            transaction_outcome: { block_hash: "BLOCKhash111" },
            receipts_outcome: [{ outcome: { logs, status: t.ok ? { SuccessValue: "" } : { Failure: {} } } }],
          };
        } else {
          const fc = { public_key: "ed25519:runner", access_key: { permission: { FunctionCall: {} } } };
          result = { keys: b.params.account_id.startsWith("cheat.") ? [{ public_key: cheatKey, access_key: { permission: "FullAccess" } }, fc] : [fc] };
        }
        return route.fulfill({ contentType: "application/json", body: JSON.stringify({ jsonrpc: "2.0", id: b.id, result }) });
      });
      const vp = await ctx.newPage();
      vp.on("pageerror", (e) => problems.push(`pageerror: ${e.message}`));
      await vp.goto(base);
      await vp.waitForSelector('#scenarios[data-ready="1"]');
      // W6: the hero's first button checks proven earnings; the second runs the cheat scenario.
      assert.equal(await vp.locator('header .cta a.primary[href="#invest-yourself"]').count(), 1, "W6: the main hero button is invest yourself");
      assert.equal(await vp.locator('header .cta a[href="#run-invest"]').count(), 1, "W6: hero button checks proven earnings");
      // F1: the 30-second screen: one-sentence lede, and the buttons sit above the picture.
      const lede = (await vp.locator("header .lede").textContent()).trim();
      assert.ok(lede.split(/[.!?]\s/).length <= 2, `F1: lede is at most two sentences: ${lede}`);
      // L1 (loop 45): the lede names the fear removed: faked revenue and a kept key.
      assert.ok(/revenue can be faked/.test(lede) && /keep a key/.test(lede), "L1: the lede names both fears");
      assert.ok(await vp.evaluate(() => document.querySelector("header .cta").getBoundingClientRect().bottom < 800), "F1: buttons above the fold at 800px");
      assert.equal(await vp.locator('header .cta a[href="#run-cheat"]').count(), 1, "W6: hero button runs the cheat scenario");
      // M1: the buyer's due-diligence cards: two buys (one refused) and one investment.
      await vp.waitForSelector("#dd-cards .dd-card");
      assert.equal(await vp.locator("#dd-cards .dd-card").count(), 3, "M1: three cards");
      assert.ok(await vp.locator("figure.deed img").evaluate((i) => i.complete && i.naturalWidth > 0), "the deed picture loads");
      assert.equal(await vp.locator('#dd-cards .dd-no[data-agent="cheat"]').count(), 1, "M1: the cheat card says do not buy");
      assert.ok(/100% of counted/.test(await vp.locator('#dd-cards [data-agent="translator3"]').textContent()), "M1: verified share shown");
      checks++;
      // V5: the hero proof strip lists the sale tx and every refused break-in call.
      await vp.waitForSelector("#proof-strip .ps-list li", { state: "attached" }); // folded under "See the 6 refused calls"
      assert.equal(await vp.locator("#proof-strip .ps-stats li a[href^=\"https://testnet.nearblocks.io/txns/\"]").count(), 3, "V5: each stat tile has its tx link");
      assert.match(await vp.locator("#proof-strip .ps-stats li").nth(2).textContent(), /6\/6/, "V5: the lockout tile counts the refused calls with a tx");
      const strip = await vp.locator("#proof-strip").textContent();
      assert.ok(/one transaction/.test(strip) && /All refused/.test(strip), "V5: proof strip text");
      assert.equal(await vp.locator("#proof-strip .ps-list li").count(), 6, "V5: six refused calls");
      assert.equal(await vp.locator('#proof-strip a[href^="https://testnet.nearblocks.io/txns/"]').count(), 10, "V5: seven tx links plus three stat links");
      assert.equal(await vp.locator("#proof-strip .ps-stats li").count(), 3, "V5: three numbers, business first");
      assert.ok(/5\/5 customer payments signed by NEAR AI/.test(strip), "V5: verified earnings first");
      checks++;
      // V4: the live shares payout, the NEAR AI check and the AI buy are runnable too.
      for (const id of ["invest", "nearai", "aibuyer"]) {
        assert.equal(await vp.locator(`[data-scenario="${id}"]`).count(), 1, `V4: scenario ${id} is listed`);
      }
      // V7: scenes 9-11 (born clean, sealed prompt, business day) appear once recorded; never as an empty card.
      for (const [id, step] of [["born", "born-clean"], ["sealed", "sealed-prompt"], ["day", "business-day"]]) {
        const want = rec.steps[step];
        assert.equal(await vp.locator(`[data-scenario="${id}"]`).count(), want ? 1 : 0, `V7: scenario ${id} shown only when recorded`);
      }
      checks++;
      // V9: the Exchange board shows only with a recorded scene 13, one row per agent, sortable.
      const ex = vp.locator("#exchange");
      if (rec.exchange) {
        await ex.waitFor({ state: "visible" });
        // One cast: the board also lists the three case-file agents, translator3 (the open offer) first.
        const dd = JSON.parse(readFileSync(new URL("../data/diligence.json", import.meta.url), "utf8"));
        assert.equal(await vp.locator("#ex-table tbody tr").count(), rec.exchange.agents.length + dd.cards.length, "V9: one row per agent");
        assert.equal(await vp.locator("#ex-table tbody tr.ex-featured td code").first().textContent(), "translator3", "V9: the open offer is on the board, first");
        const first = await vp.locator("#ex-table tbody tr td code").first().textContent();
        await vp.click('#ex-table th[data-k="name"]');
        const sorted = await vp.locator("#ex-table tbody tr td code").allTextContents();
        assert.deepEqual(sorted, [...sorted].sort(), "V9: sorting by name works");
        assert.ok(first, "V9: rows have names");
        assert.match(await vp.locator("#ex-table tbody tr.ex-featured .ex-early").textContent(), /early: 1 day of revenue/, "E1: the featured row says the multiple is early");
      } else {
        assert.equal(await ex.isVisible(), false, "V9: hidden with no exchange");
      }
      checks++;
      // V8: "Invest yourself" shows only with a recorded open offer (scene 12), with 5 steps of commands naming that agent.
      const iy = vp.locator("#invest-yourself");
      if (rec.open_offer) {
        await iy.waitFor({ state: "visible" });
        assert.equal(await vp.locator("#iy-steps > li").count(), 5, "V8: five invest steps");
        assert.ok((await iy.textContent()).includes(rec.open_offer.agent), "V8: commands name the offer's agent");
        // I1 (loop 45): "Investors so far" lists the MCP agent investor and the page's Invest tx, each labelled.
        const inv = await vp.locator("#iy-investors li").allTextContents();
        for (const h of rec.open_offer.offer.holders) assert.ok(inv.some((t) => t.includes(h.account) && /MCP agent/.test(t)), `I1: ${h.account} labelled as the MCP agent investor`);
        if (rec.open_offer.example_tx) assert.ok(inv.some((t) => t.includes(rec.open_offer.example_tx.account) && /Invest button/.test(t) && /signed by script/.test(t)), "I1: the Invest-button tx is listed and labelled");
        // E1 (loop 45): an honest note on the early multiple, from the data.
        assert.match(await vp.locator("#iy-early").textContent(), /^Early: 1 day of revenue \(50\.00\)\. The price is 250\.00 for 25% of earnings until 30 Nov 2026\.$/, "E1: early note on the invest card");
        checks += 3;
        // W9: one-click wallet links. Disabled until a valid account is typed; then the link is
        // MyNearWallet's /sign page with one transaction (faucet + buy) signed by that account.
        assert.equal(await vp.locator("#iy-invest").getAttribute("aria-disabled"), "true", "W9: invest link disabled with no account");
        await vp.fill("#iy-account", "Bad Name!");
        assert.equal(await vp.locator("#iy-invest").getAttribute("aria-disabled"), "true", "W9: invalid account keeps it disabled");
        // W11: a .near name says why nothing happens and where to get a testnet account.
        await vp.fill("#iy-account", "softlynx.near");
        assert.equal(await vp.locator("#iy-invest").getAttribute("aria-disabled"), "true", "W11: a .near account keeps it disabled");
        assert.match(await vp.locator("#iy-out").textContent(), /real-money \.near account.*\.testnet/, "W11: a .near account gets a testnet hint");
        await vp.fill("#iy-account", "judge.testnet");
        const href = await vp.locator("#iy-invest").getAttribute("href");
        assert.ok(href.startsWith(`${WALLET_SIGN}?transactions=`), "W9: invest link goes to MyNearWallet sign");
        const u = new URL(href);
        assert.equal(u.searchParams.get("transactions"), base64(encodeTx(investTx(rec.open_offer, "judge.testnet"))), "W9: the link carries the faucet + buy transaction");
        assert.ok(new URL(u.searchParams.get("callbackUrl")).searchParams.get("iy") === "judge.testnet", "W9: wallet returns to the page");
        assert.ok((await vp.locator("#iy-claim").getAttribute("href")).includes("transactions="), "W9: claim link built");
      } else {
        assert.equal(await iy.isVisible(), false, "V8: hidden with no open offer");
      }
      checks++;
      // F2: "How it works" is folded; a link into it opens it.
      assert.equal(await vp.evaluate(() => document.getElementById("how-it-works").open), false, "F2: folded by default");
      await vp.evaluate(() => { location.hash = "#backend"; });
      await vp.waitForFunction(() => document.getElementById("how-it-works").open);
      await vp.evaluate(() => { location.hash = ""; });
      checks++;
      for (const id of ["sale", "cheat"]) {
        await vp.click(`[data-scenario="${id}"] > summary`);
        await vp.click(`[data-run="${id}"]`);
        await vp.waitForFunction((sid) => /Run again/.test(document.querySelector(`[data-run="${sid}"]`).textContent), id);
      }
      const pills = await vp.locator(".vpill").allTextContents();
      const steps = ["honest-sale", "break-in", "cheat"].map((k) => rec.steps[k]);
      const txStages = steps.flatMap((x) => x.txs).filter((t) => t.tx).length;
      const scans = steps.filter((x) => x.scan).length;
      const confirmed = pills.filter((p) => p.includes("confirmed")).length;
      const differs = pills.filter((p) => p.includes("differs")).length;
      if (!lie) {
        assert.equal(confirmed, txStages + scans, `V1: every recorded tx and the scan confirmed (${pills.join(" | ")})`);
        assert.ok(pills.filter((p) => p === "refused (as expected)").length >= 2, "V2: node-refused keys say 'refused (as expected)'");
        assert.ok(asked.includes("tx") && asked.includes("query"), "V1: the page asked the chain");
      } else {
        assert.equal(differs, txStages, `V3: a different signer on chain shows as a mismatch (${differs}/${txStages}: ${pills.join(" | ")})`);
      }
      // W6 (review 5): each tx stage offers a curl to a public archival node, not our relay.
      const curls = await vp.locator(".selfcheck pre code").allTextContents();
      assert.equal(curls.length, txStages, "W6: one self-check per tx stage");
      assert.ok(curls.every((c) => c.includes("https://archival-rpc.testnet.near.org") && /"method":"tx"/.test(c)), "W6: curl to the public archival node");
      await noHorizontalScroll(vp, "scenario runner at 375px");
      checks += 4;
      await ctx.close();
    }
    const { refuse } = await import("../rpc-allow.mjs");
    const tx = (p) => ({ jsonrpc: "2.0", id: 1, method: "tx", params: p });
    assert.equal(refuse(tx({ tx_hash: "GAPESp9FLYhSNgomeJDDgx6mPTCfsYxWrSCzz8V4eC3K", sender_account_id: "bob.agentexit3.testnet", wait_until: "NONE" })), null);
    assert.ok(refuse(tx({ signed_tx_base64: "AAAA", wait_until: "NONE" })), "V4: a signed tx is never relayed");
    assert.ok(refuse(tx({ tx_hash: "GAPESp9FLYhSNgomeJDDgx6mPTCfsYxWrSCzz8V4eC3K", sender_account_id: "bob.near", wait_until: "NONE" })), "V4: mainnet sender refused");
    checks += 3;
  }

  // U1-U4 (redesign): a short first screen with one main action, sections in reading order, no sideways scroll.
  for (const vp of [{ width: 1280, height: 800 }, { width: 375, height: 812 }]) {
    const q = await page(vp);
    await q.goto(base);
    await q.waitForSelector('#scenarios[data-ready="1"]');
    await q.waitForSelector('#play[data-ready="1"]');
    await q.waitForSelector("#dd-cards .dd-card");
    await q.waitForSelector("#proof-strip .ps-stats li");
    await q.locator("#exchange").waitFor({ state: "visible" });
    await q.locator("#invest-yourself").waitFor({ state: "visible" });
    assert.ok((await q.locator("header .cta .btn").count()) <= 2, `U1: at most 2 hero buttons at ${vp.width}px`);
    assert.equal(await q.locator("header .cta .btn.primary").count(), 1, "U1: one primary hero button");
    const words = await q.evaluate(() => document.body.innerText.split(/\s+/).filter((w) => /\w/.test(w)).length);
    // Was 1,808 before the redesign. What is left is mostly UI: the table, the three case-file
    // scorecards and the playground's live state; prose sits in closed folds.
    // Loop 45 added the business lines judges asked for (fee, running cost, first users, deal splits)
    // and the judge command block: about 100 words, so the budget moved from 740 to 850.
    // Loop 45 round 3 added what judges asked for: the two-sentence lede naming both fears, labelled
    // investors, the early-multiple note and why each playground step is disabled: about 65 words.
    // Loop 46: the "Built on NEAR" strip for judges (7 rows, each with its tx, and the link to the
    // standard) adds about 85 words, so the budget moved from 910 to 1000.
    assert.ok(words <= 1000, `U2: ${words} visible words on load at ${vp.width}px (budget 1000)`);
    const order = await q.evaluate(() => ["exchange", "invest-yourself", "protect", "play", "evidence-run", "check", "for-judges"].map((id) => {
      const e = document.getElementById(id); return e ? e.getBoundingClientRect().top + scrollY : -1; }));
    assert.ok(order.every((y, i) => y >= 0 && (i === 0 || y > order[i - 1])), `U3: sections in order (${order.join(", ")})`);
    assert.equal(await q.evaluate(() => document.getElementById("replay").open), false, "U3: the offline replay is folded on a plain load");
    const h = await q.evaluate(() => document.documentElement.scrollHeight);
    // Loop 45 round 3: the same additions (step reasons, investor labels, early note) cost ~120px on desktop, ~330px at 375px.
    // Loop 46: the "Built on NEAR" strip costs ~235px on desktop and ~440px at 375px (each row wraps to two lines).
    // Budgets keep ~8% headroom: system fonts differ by OS (Linux 8,265px vs Windows/WSL 8,575px at 375px for the same page).
    assert.ok(h <= (vp.width > 600 ? 4800 : 9200), `U4: page is ${h}px tall at ${vp.width}px`);
    const banned = await q.evaluate(() => {
      const judges = document.getElementById("for-judges").innerText + document.getElementById("ex-note").innerText + document.querySelector("footer").innerText;
      return (document.body.innerText.match(/NotOwner|NotRunner|NEP-\d|nft_token|0xfe|born clean|age-weighted|escrow floor/g) || []).filter((w) => !judges.includes(w));
    });
    assert.deepEqual(banned, [], `U4: no code names in visible text at ${vp.width}px`);
    await noHorizontalScroll(q, `whole page at ${vp.width}px`);
    checks += 7;
    await q.close();
  }

  // B1-B9 (loop 45): business first, numbers from the files, one proof section, a folded judge grid.
  {
    const world = JSON.parse(readFileSync(new URL("../data/testnet.json", import.meta.url), "utf8"));
    const web = new URL("../", import.meta.url);
    // B1: the page never says "mainnet": its own HTML, scripts and data (not the vendored wallet library).
    const own = [...readdirSync(web).filter((f) => /\.(html|js|mjs|css)$/.test(f)), ...readdirSync(new URL("data/", web)).map((f) => `data/${f}`)];
    for (const f of own) { assert.ok(!/mainnet/i.test(readFileSync(new URL(f, web), "utf8")), `B1: "mainnet" in web/${f}`); checks++; }
    // B2: the hero payout is computed from the recorded run: 14.00 of 70.00 (20%).
    const pay = investorPayout(world);
    assert.ok(pay, "B2: the invest run is recorded");
    assert.equal(pay.total, 7000, "B2: the recorded customer payments add up to 70.00");
    assert.equal(pay.investor, (pay.total * pay.pct) / 100, "B2: the investor gets exactly their share");
    assert.equal(`${fmt(pay.investor)} of ${fmt(pay.total)}`, "14.00 of 70.00");
    // B3: the deal waterfalls add up to the price.
    for (const [kind, price, want] of [["sale", 100000, [69000, 1000, 30000]], ["share", 50000, [24500, 500, 25000]]]) {
      const w = waterfall(kind, price);
      assert.deepEqual([w.seller, w.fee, w.held], want, `B3: ${kind} split`);
      assert.equal(w.seller + w.fee + w.held, price, `B3: ${kind} parts add up`);
    }
    // B4: numbers in the docs match the files (drift check).
    const repo = new URL("../../", import.meta.url);
    const wasmBytes = statSync(new URL("contracts/res/agent_exit.wasm", repo)).size;
    const bytes = wasmBytes.toLocaleString("en-US");
    for (const f of ["README.md", "docs/LIMITS.md"]) assert.ok(readFileSync(new URL(f, repo), "utf8").includes(`${bytes} bytes`), `B4: ${f} names the wasm size (${bytes} bytes)`);
    const html = readFileSync(new URL("index.html", web), "utf8");
    const rsFiles = execFileSync("git", ["ls-files", "*.rs"], { cwd: repo.pathname, encoding: "utf8" }).split("\n").filter(Boolean);
    const rustTests = rsFiles.reduce((n, f) => n + (readFileSync(new URL(f, repo), "utf8").match(/#\[test\]/g) || []).length, 0);
    const mcpTests = Number(/\((\d+) tests, then/.exec(readFileSync(new URL("docs/COMMANDS.md", repo), "utf8"))[1]);
    assert.ok(html.includes(`${rustTests} Rust tests and ${mcpTests} MCP tests`), `B4: the page says ${rustTests} Rust tests and ${mcpTests} MCP tests`);
    assert.ok(!/\d+\+ tests/.test(html), "B4: no rounded test count");
    checks += 6;
    // E1/I1 (loop 45): the early-multiple note and the investor labels, from the data.
    {
      const dd = JSON.parse(readFileSync(new URL("data/diligence.json", web), "utf8"));
      const t3 = dd.cards.find((c) => c.agent === world.open_offer.agent);
      assert.equal(t3.revenue_days, 1, "E1: translator3's counted revenue came in on one day (scene 8)");
      const n = earlyNote({ days: 1, counted: 5000, price: 25000, bps: 2500, end: Date.UTC(2026, 10, 30, 23, 59, 59) });
      assert.equal(n.multiple, "20.0×");
      assert.equal(n.short, "early: 1 day of revenue");
      assert.equal(n.text, "Early: 1 day of revenue (50.00). The price is 250.00 for 25% of earnings until 30 Nov 2026.");
      assert.equal(earlyNote({ days: 3, counted: 5000, price: 25000, bps: 2500, end: 0 }).short, "early: 3 days of revenue");
      assert.equal(earlyNote({ days: 40, counted: 5000, price: 25000, bps: 2500, end: 0 }), null, "E1: no note once there is a month of revenue");
      const list = investorList(world.open_offer);
      assert.deepEqual(list.map((x) => x.account), ["carol.agentexit5.testnet", "p6.agentexit5.testnet"]);
      assert.match(list[0].label, /MCP agent investor/);
      assert.match(list[1].label, /Invest button tx, signed by script/);
      checks += 3;
    }
    // B5: the factory link is the recorded scene-13 birth.
    const scene13 = readFileSync(new URL("out/scenes/scene-13.json", repo), "utf8");
    const factoryTx = /txns\/(\w+)" target="_blank" rel="noopener noreferrer" id="factory-tx"/.exec(html)[1];
    assert.ok(scene13.includes(factoryTx), "B5: the factory tx is in out/scenes/scene-13.json");
    checks++;
    for (const vp of [{ width: 1280, height: 800 }, { width: 375, height: 812 }]) {
      const q = await page(vp);
      await q.goto(base);
      await q.waitForSelector("#proof-strip .ps-stats li");
      await q.waitForSelector("#waterfall .wf-deal");
      await q.waitForSelector('#scenarios[data-ready="1"]');
      const body = await q.evaluate(() => document.body.textContent);
      assert.ok(!/mainnet/i.test(body), `B1: rendered page says "mainnet" at ${vp.width}px`);
      const stat = await q.locator("#proof-strip .ps-stats li").nth(1);
      assert.match(await stat.textContent(), /14\.00 of 70\.00\s*paid to the investor \(20%\)/, "B2: hero payout stat");
      assert.equal(await stat.locator("a").getAttribute("href"), pay.claim.explorer, "B2: the stat links the claim tx");
      const hero = await q.textContent("header.hero");
      assert.ok(hero.includes("How it earns: a 1% fee on every sale and share buy, taken in the same transaction"), "B2: business model line");
      assert.ok(/no server of ours touches the money/.test(hero), "B2: running cost line");
      assert.equal(await q.locator('header.hero a[href$="docs/BUSINESS.md"]').count(), 1, "B2: links BUSINESS.md");
      assert.equal(await q.locator('header.hero a', { hasText: "Source code" }).count(), 1, "B9: source code link in the hero");
      const ex = await q.textContent("#exchange");
      for (const t of ["translator sold for 1,000.00, so the fee was 10.00", "13 MCP tools", "Only on NEAR"]) assert.ok(ex.includes(t), `B3: Exchange says "${t}"`);
      const sums = await q.$$eval("#waterfall .wf-deal", (ds) => ds.map((d) => [Number(d.dataset.total), [...d.querySelectorAll("[data-cents]")].reduce((a, e) => a + Number(e.dataset.cents), 0)]));
      assert.deepEqual(sums, [[100000, 100000], [50000, 50000]], "B3: waterfall parts add up on the page");
      // B6: one proof section; no duplicate limits text, no tiles, no eyebrows, no arrows on links.
      assert.equal(await q.locator("#evidence-run #check").count(), 1, "B6: the scan is inside the proof section");
      for (const sel of ["#evidence", "#concept", ".eyebrow", "header .tag"]) assert.equal(await q.locator(sel).count(), 0, `B6: ${sel} removed`);
      assert.equal((body.match(/regression suite, not independent evidence/g) || []).length, 1, "B6: honest limits said once");
      assert.equal(await q.locator("header .cta a.primary").textContent(), "Buy a 2.5% share for 25.00", "B9: the main button names the deal");
      // B7: the judge grid is folded; the command block can be copied; the roadmap has three steps.
      assert.equal(await q.evaluate(() => document.getElementById("criteria").open), false, "B7: criteria folded");
      assert.equal(await q.locator("#for-judges #criteria .crit a").count(), 5, "B7: five criteria inside the fold");
      assert.equal(await q.locator("#copy-judge").count(), 1, "B7: copy button");
      assert.match(await q.textContent("#judge-cmd"), /make judge/, "B7: the command runs make judge");
      assert.equal(await q.locator("#for-judges .roadmap li").count(), 3, "B7: three roadmap steps");
      assert.ok((await q.textContent("#for-judges")).includes("/mcp__agent-exit__judge_tour"), "B7: names the judge_tour prompt");
      await noHorizontalScroll(q, `loop 45 page at ${vp.width}px`);
      checks += 20;
      await q.close();
    }
  }

  // W11: back from the wallet, the transaction shows on the page and stays listed (localStorage).
  {
    const world = JSON.parse(readFileSync(new URL("../data/testnet.json", import.meta.url), "utf8"));
    if (world.open_offer) {
      const o = world.open_offer;
      const ctx = await browser.newContext({ viewport: { width: 375, height: 812 } });
      await ctx.route("**/*", async (route) => {
        const u = route.request().url();
        if (!u.startsWith(base)) return route.abort();
        if (!u.startsWith(`${base}api/rpc`)) return route.continue();
        const b = JSON.parse(route.request().postData());
        const value = b.params.method_name === "get_holdings" ? { holdings: [{ bps: 250 }], claimable: "1500000" }
          : [{ ...o.offer, sold_bps: 500, holders: [...o.offer.holders, { account: "judge.testnet", bps: 250 }] }];
        return route.fulfill({ contentType: "application/json", body: JSON.stringify({ jsonrpc: "2.0", id: b.id, result: { result: [...new TextEncoder().encode(JSON.stringify(value))] } }) });
      });
      const wp = await ctx.newPage();
      wp.on("pageerror", (e) => problems.push(`pageerror: ${e.message}`));
      const h = "D4xQARvNcaL4euHzvMVWFBt7gg8SnYQGHPuiPbD1ML7B";
      await wp.goto(`${base}?iy=judge.testnet&transactionHashes=${h}`);
      await wp.locator("#iy-receipt").waitFor({ state: "visible" });
      assert.equal(await wp.locator(`#iy-receipt a[href="https://testnet.nearblocks.io/txns/${h}"]`).count(), 1, "W11: the receipt links the tx");
      await wp.waitForFunction(() => /1\.50/.test(document.getElementById("iy-receipt").textContent));
      assert.ok((await wp.textContent("#iy-investors")).includes("judge.testnet · 2.5%"), "W11: listed among investors");
      await wp.waitForFunction(() => /2 of 10 parts sold/.test(document.getElementById("iy-live").textContent));
      await wp.goto(base);
      await wp.locator("#invest-yourself").waitFor({ state: "visible" });
      await wp.waitForFunction(() => document.getElementById("iy-investors").textContent.includes("judge.testnet"));
      assert.ok((await wp.locator('#iy-investors a[href^="https://testnet.nearblocks.io/txns/"]').count()) >= 1, "W11: still listed after a reload");
      // A cancelled signature changes nothing.
      await wp.goto(`${base}?iy=judge.testnet&errorCode=userRejected`);
      await wp.waitForFunction(() => /did not send/.test(document.getElementById("iy-out").textContent));
      assert.equal(await wp.locator("#iy-receipt").isVisible(), false, "W11: no receipt when the wallet sent nothing");
      // The invest button looks disabled (grey) with no account; a click still asks for one instead of going nowhere.
      await wp.goto(base);
      await wp.locator("#invest-yourself").waitFor({ state: "visible" });
      await wp.click("#iy-invest", { force: true }); // Playwright waits on aria-disabled; a person can still click it
      assert.match(await wp.textContent("#iy-out"), /Type your testnet account first/, "W11: a click with no account says what to do");
      assert.ok(await wp.evaluate(() => document.activeElement.id === "iy-account"), "W11: and focuses the account box");
      checks += 6;
      await ctx.close();
    }
  }

  // D1: deep links open their fold.
  {
    const ctx = await browser.newContext({ viewport: { width: 1280, height: 800 } });
    // #run-day runs the scenario: answer the relay with an error instead of reaching for the network.
    await ctx.route("**/*", (r) => { const u = r.request().url(); if (!u.startsWith(base)) return r.abort(); if (u.startsWith(`${base}api/rpc`)) return r.fulfill({ contentType: "application/json", body: '{"jsonrpc":"2.0","id":1,"error":{"message":"offline test"}}' }); return r.continue(); });
    const q = await ctx.newPage();
    q.on("pageerror", (e) => problems.push(`pageerror: ${e.message}`));
    for (const [hash, fold] of [["#test-count", "criteria"], ["#backend", "how-it-works"], ["#break-in", "replay"], ["#run-day", "sc-more"]]) {
      await q.goto(`${base}${hash}`);
      if (fold === "sc-more") await q.waitForFunction(() => document.querySelector(".sc-more")?.open === true);
      else if (fold) await q.waitForFunction((id) => document.getElementById(id).open, fold);
      else await q.locator(hash).waitFor({ state: "visible" });
      checks++;
    }
    await q.goto(`${base}?network=testnet`);
    await q.waitForFunction(() => document.getElementById("replay").open);
    checks++;
    await ctx.close();
  }

  // N1 (loop 45): at 375px every nav anchor stays reachable (the row scrolls sideways; the page does not).
  {
    const q = await page({ width: 375, height: 740 });
    await q.goto(base);
    await q.waitForSelector('#scenarios[data-ready="1"]');
    const hrefs = await q.locator('nav.top a[href^="#"]:not(.brand)').evaluateAll((as) => as.map((a) => a.getAttribute("href")));
    assert.deepEqual(hrefs, ["#exchange", "#invest-yourself", "#protect", "#evidence-run", "#for-judges"], "N1: nav anchors");
    for (const h of hrefs) {
      const a = q.locator(`nav.top a[href="${h}"]`);
      assert.ok(await a.evaluate((x) => getComputedStyle(x).display !== "none"), `N1: ${h} shown at 375px`);
      await a.click();
      await q.waitForFunction((want) => location.hash === want, h);
      checks++;
    }
    await noHorizontalScroll(q, "nav at 375px");
    await q.close();
  }

  assert.deepEqual(problems, [], "no console errors");
  console.log(`web-test: ${checks} checks passed (desktop + 375px, offline + testnet flag)`);
} finally {
  await browser.close();
  server.close();
}
