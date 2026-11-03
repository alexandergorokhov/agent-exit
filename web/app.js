// Agent Exit demo page. Offline by default: every result shown is read from
// data/demo.json, which tools/verify/src/bin/gen-web-demo.rs records from the
// contract in near-sdk's mocked runtime. `?network=testnet` shows the testnet
// mode: the recorded testnet run of 2026-11-03 with explorer links, plus read-only live reads.

// Vendored and hash-pinned (web/vendor/SHA256SUMS); the page CSP allows no remote script.
const NEAR_CONNECT = "./vendor/near-connect-0.11.4.esm.js";
const params = new URLSearchParams(location.search);
const network = params.get("network") === "testnet" ? "testnet" : "offline";

function el(tag, attrs = {}, ...kids) {
  const n = document.createElement(tag);
  for (const [k, v] of Object.entries(attrs)) {
    if (v == null) continue;
    if (k === "class") n.className = v;
    else if (k === "text") n.textContent = v;
    else n.setAttribute(k, v);
  }
  for (const k of kids.flat()) if (k != null) n.append(k.nodeType ? k : String(k));
  return n;
}

const json = (v) => JSON.stringify(v, null, 2);

function jsonBlock(label, value, open = false) {
  const d = el("details", open ? { open: "" } : {}, el("summary", { text: label }), el("pre", { text: json(value) }));
  return d;
}

function badge(kind, label) {
  return el("span", { class: `badge ${kind}`, text: label || kind });
}

function callCard(c) {
  const ok = c.outcome === "ok";
  // display: how the page labels a call that returned without panicking but is not a plain success.
  const kind = c.display || c.outcome;
  const li = el("li", { class: "call", "data-outcome": kind },
    el("div", { class: "call-head" }, badge(kind, c.label), el("code", { text: c.call })),
    el("div", { class: "by", text: `by ${c.by}` }));
  if (!ok) li.append(el("p", { class: "err", text: c.error }));
  if (c.note) li.append(el("p", { class: "note", text: c.note }));
  const facts = el("ul", { class: "facts" });
  for (const e of c.events || []) facts.append(el("li", {}, "event ", el("code", { text: e.event })));
  for (const k of c.key_actions || []) facts.append(el("li", {}, `key ${k.action} `, el("code", { text: k.key })));
  for (const t of c.token_transfers || []) facts.append(el("li", {}, `transfer ${t.amount} to `, el("code", { text: t.receiver_id || t.to || "" })));
  if (facts.childElementCount) li.append(facts);
  if (c.events && c.events.length) li.append(jsonBlock("Event JSON (NEP-297 logs)", c.events));
  if (ok && c.result !== undefined) li.append(jsonBlock("Return value", c.result));
  return li;
}

function mcpCard(m, i) {
  const li = el("li", { class: "call", "data-outcome": m.ok ? "ok" : "error" },
    el("div", { class: "call-head" }, el("span", { class: "by", text: `${i + 1}.` }), badge(m.ok ? "ok" : "error"), el("code", { text: `${m.tool}(${JSON.stringify(m.args)})` })),
    el("div", { class: "by", text: m.why }));
  if (!m.ok) li.append(el("p", { class: "err", text: `${m.error.code}: ${m.error.message}` }));
  li.append(jsonBlock(m.ok ? "Tool response" : "Tool error", m.ok ? m.response : m.error));
  return li;
}

function scanBlock(scan) {
  const box = el("div", {}, el("h3", { text: `Buyer's scan: verdict ${scan.verdict}` }));
  const ul = el("ul", { class: "find" });
  for (const f of scan.findings) {
    ul.append(el("li", { class: f.severity === "block" ? "" : "warn" }, el("code", { text: f.code }), ` (${f.severity}) `, f.detail));
  }
  box.append(ul, jsonBlock("Scan report JSON", scan));
  return box;
}

let demo;
let testnet = null;
let current = 0;

const TESTNET_LABEL = "testnet: run of 2026-11-03 (recorded; \"Read live\" checks the chain now)";

// ----- live reads (testnet mode only, on click) ------------------------------
// The CSP allows only 'self', so reads go through the read-only relay at
// api/rpc (web/rpc-allow.mjs): view queries only, nothing is signed or sent.

async function rpcQuery(params) {
  const r = await fetch("api/rpc", { method: "POST", headers: { "content-type": "application/json" },
    body: JSON.stringify({ jsonrpc: "2.0", id: "page", method: "query", params: { finality: "final", ...params } }) });
  const j = await r.json();
  if (j.error) throw new Error(j.error.message || j.error.name || JSON.stringify(j.error));
  if (j.result && j.result.error) throw new Error(j.result.error);
  return j.result;
}

async function viewCall(account, method) {
  const res = await rpcQuery({ request_type: "call_function", account_id: account, method_name: method, args_base64: btoa("{}") });
  return JSON.parse(new TextDecoder().decode(new Uint8Array(res.result)));
}

async function readAgent(account) {
  const [acct, keys, status] = await Promise.all([
    rpcQuery({ request_type: "view_account", account_id: account }),
    rpcQuery({ request_type: "view_access_key_list", account_id: account }),
    viewCall(account, "get_status"),
  ]);
  const full = keys.keys.filter((k) => k.access_key.permission === "FullAccess").length;
  return { account, code_hash: acct.code_hash, owner: status.owner, status: status.status, treasury: status.treasury, keys: keys.keys.length, full };
}

async function liveRead(out) {
  const agents = ["translator", "cheat"].map((a) => `${a}.${testnet.root}`);
  out.replaceChildren(el("p", { class: "muted", text: `Reading ${agents.join(" and ")} from NEAR testnet…` }));
  try {
    const rows = await Promise.all(agents.map(readAgent));
    const same = (h) => h === testnet.agent_code_hash ? "same as the recorded run" : "differs from the recorded run (redeployed since)";
    out.replaceChildren(
      el("h3", { text: `Live from NEAR testnet, ${new Date().toISOString().slice(0, 19)}Z` }),
      el("ul", { class: "find live" }, rows.map((r) => el("li", {},
        el("code", { text: r.account }), ` owner ${r.owner}, ${r.status}, treasury ${r.treasury ?? "?"}; `,
        `${r.keys} access keys, ${r.full} full-access; code hash `, el("code", { text: r.code_hash }), ` (${same(r.code_hash)})`))),
      el("p", { class: "muted", text: "Read-only view calls through the page's relay. The cheat agent is expected to show a full-access key: that is what the buyer's scan blocks." }));
  } catch (e) {
    out.replaceChildren(el("p", { class: "err", text: `Live read failed: ${e && e.message ? e.message : e}` }));
  }
}

function liveButton() {
  const out = el("div", { id: "live" });
  const b = el("button", { type: "button", id: "read-live", text: "Read live" });
  b.addEventListener("click", async () => { b.disabled = true; await liveRead(out); b.disabled = false; });
  return [b, out];
}

function testnetBlock(id) {
  const t = testnet && testnet.steps[id];
  if (!t) return null;
  const items = t.txs.map((x) => el("li", { class: x.ok ? "" : "warn" },
    el("span", { class: `badge ${x.ok ? "ok" : "error"}`, text: x.ok ? "ok" : "refused" }), " ", x.label,
    x.error ? el("code", { text: ` ${x.error}` }) : "",
    x.explorer ? el("span", {}, " · ", el("a", { href: x.explorer, target: "_blank", rel: "noopener noreferrer", text: "NearBlocks" })) : ""));
  const kids = [el("h3", { text: `On NEAR testnet (scene ${t.scene}, ${t.pass ? "passed" : "failed"})` })];
  if (items.length) kids.push(el("ul", { class: "find" }, items));
  else kids.push(el("p", { class: "muted", text: "Read-only step: the recorded views are in out/scenes." }));
  return el("div", { class: "testnet" }, ...kids);
}

function renderNav() {
  const ol = document.getElementById("steps");
  ol.replaceChildren(...demo.steps.map((s, i) =>
    el("li", {}, el("a", { href: `#${s.id}`, "aria-current": i === current ? "step" : null },
      el("span", { class: "n", text: String(i + 1) }), el("span", { text: s.title })))));
}

function render() {
  const s = demo.steps[current];
  const sec = document.getElementById("step");
  sec.dataset.step = s.id;
  const stats = el("div", { class: "stats" }, s.headline.map((h) =>
    el("div", { class: "stat" }, el("div", { class: "v", text: h.value }), el("div", { class: "l", text: h.label }))));
  const kids = [
    el("h2", { text: `${current + 1}. ${s.title}` }),
    el("p", { class: "ids", text: `Tests behind this step: ${s.scenarios.join(", ")}` }),
    el("p", { class: "story", text: s.story }),
    stats,
  ];
  if (s.calls && s.calls.length) {
    kids.push(el("h3", { text: "Contract calls, as recorded" }), el("ol", { class: "calls" }, s.calls.map(callCard)));
  }
  if (s.scan) kids.push(scanBlock(s.scan));
  if (s.listing) kids.push(el("h3", { text: "The listing bob's scan saw" }), jsonBlock("get_listing()", s.listing));
  if (s.mcp_steps) kids.push(el("h3", { text: "MCP tool calls by the summarizer agent" }), el("ol", { class: "calls" }, s.mcp_steps.map(mcpCard)));
  if (s.slot) {
    const slot = el("div", { class: "slot", "data-slot": "ai-buyer-transcript" }, el("h3", { text: `${s.slot.title} (${s.slot.status})` }), el("p", { class: "muted", text: s.slot.text }));
    if (s.slot.steps && s.slot.steps.length) {
      slot.append(el("ol", { class: "calls ai" }, s.slot.steps.map((st) => el("li", { class: "call" },
        el("p", { class: "said", text: st.said }),
        el("ul", { class: "find" }, st.calls.map((c) => el("li", {},
          el("code", { text: `${c.tool}(${JSON.stringify(c.args)})` }), ` → ${c.outcome}`)))))));
    }
    kids.push(slot);
  }
  if (s.view) kids.push(el("h3", { text: "View" }), jsonBlock(s.view.name, s.view.value));
  const tn = testnetBlock(s.id);
  if (tn) kids.push(tn);
  sec.replaceChildren(...kids);
  document.getElementById("prev").disabled = current === 0;
  document.getElementById("next").disabled = current === demo.steps.length - 1;
  renderNav();
}

function go(i, push = true) {
  current = Math.max(0, Math.min(demo.steps.length - 1, i));
  if (push) history.replaceState(null, "", `${location.search}#${demo.steps[current].id}`);
  render();
}

function fromHash() {
  const i = demo.steps.findIndex((s) => `#${s.id}` === location.hash);
  return i < 0 ? 0 : i;
}

function renderMode() {
  const box = document.getElementById("mode");
  if (network === "offline") {
    box.replaceChildren(
      el("span", { class: "pill", text: "offline: recorded results" }),
      el("a", { href: "?network=testnet", text: "testnet mode" }));
    return;
  }
  const who = el("span", { class: "muted", id: "wallet" });
  if (!TESTNET_LIVE) {
    box.replaceChildren(
      el("span", { class: "pill warn", id: "testnet-label", text: TESTNET_LABEL }),
      ...(testnet ? liveButton() : []),
      el("a", { href: "./", text: "back to offline" }));
    return;
  }
  const login = el("button", { type: "button", id: "login", text: "Log in with NEAR wallet" });
  login.addEventListener("click", async () => {
    login.disabled = true;
    who.textContent = "Opening wallet selector…";
    try {
      const { NearConnector } = await import(NEAR_CONNECT);
      const connector = new NearConnector({ network: "testnet" });
      connector.on("wallet:signIn", (t) => { who.textContent = `Signed in as ${t.accounts[0].accountId} (nothing is sent yet).`; });
      await connector.connect();
    } catch (e) {
      who.textContent = `Wallet login failed (the page's strict CSP blocks wallet hosts until they are allow-listed): ${e && e.message ? e.message : e}`;
    } finally {
      login.disabled = false;
    }
  });
  box.replaceChildren(
    el("span", { class: "pill warn", id: "testnet-label", text: TESTNET_LABEL }),
    login, who,
    el("a", { href: "./", text: "back to offline" }));
}

// Flip when the testnet deploy is live and the wallet hosts are allow-listed (CSP).
const TESTNET_LIVE = false;

async function main() {
  renderMode();
  const res = await fetch("data/demo.json");
  if (!res.ok) throw new Error(`data/demo.json: HTTP ${res.status}`);
  demo = await res.json();
  if (demo.schema !== "web-demo.v1") throw new Error(`unknown schema ${demo.schema}`);
  try {
    const t = await fetch("data/testnet.json");
    if (t.ok) { const j = await t.json(); if (j.schema === "web-testnet.v1") testnet = j; }
  } catch { /* the testnet run is optional on the page */ }
  renderMode();
  document.getElementById("source").textContent = `Data: ${demo.source}${testnet ? `, plus the NEAR testnet run of ${testnet.run} (${testnet.root}, agent code hash ${testnet.agent_code_hash})` : ""}. All results are recorded; in testnet mode \"Read live\" makes read-only view calls.`;
  document.getElementById("prev").addEventListener("click", () => go(current - 1));
  document.getElementById("next").addEventListener("click", () => go(current + 1));
  addEventListener("hashchange", () => go(fromHash(), false));
  go(fromHash(), false);
}

main().catch((e) => {
  document.getElementById("step").replaceChildren(el("p", { class: "err", text: `Could not load the demo: ${e.message}` }));
});
