// Agent Exit demo page. Offline by default: every result shown is read from
// data/demo.json, which tools/verify/src/bin/gen-web-demo.rs records from the
// contract in near-sdk's mocked runtime. `?network=testnet` shows the testnet
// mode: the recorded testnet run of 2026-11-11 (agentexit5.testnet) with explorer links, plus read-only live reads.

// Vendored and hash-pinned (web/vendor/SHA256SUMS); the page CSP allows no remote script.
const NEAR_CONNECT = "./vendor/near-connect-0.11.4.esm.js";
import { signUrl, investTx, claimTx, validAccount } from "./wallet-link.mjs";
import { fmt, investorPayout, waterfall, earlyNote, investorList } from "./ledger.mjs";
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

const TESTNET_LABEL = "testnet: agentexit5, run of 2026-11-11 (recorded; \"Read live\" checks the chain now)";

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
      el("p", { class: "muted", text: "Read-only view calls through the page's relay. The cheat agent is expected to show a full-access key: that is what the buyer's scan flags." }));
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
  const head = el("div", { class: "tn-head" },
    el("span", { class: `dot ${t.pass ? "ok" : "bad"}` }),
    el("b", { text: `Same step on NEAR testnet: ${t.pass ? "passed" : "failed"}` }),
    el("span", { class: "muted", text: items.length ? ` · ${items.length} transactions` : " · read-only" }));
  const kids = [head];
  if (items.length) kids.push(el("details", {}, el("summary", { text: `Show the ${items.length} NearBlocks links (scene ${t.scene})` }), el("ul", { class: "find" }, items)));
  return el("div", { class: "testnet" }, ...kids);
}

// One plain sentence per step; the full recorded story sits behind "Read more".
const SHORT = {
  "break-in": "The old seller tries every way back in. Every call is refused.",
  business: "The agent pays for its work, but only inside the owner's daily limits.",
  verified: "Only payments from real customers count. NEAR AI-signed work gets a Verified badge.",
  cheat: "A seller hides a backdoor key. The buyer's scan finds it and blocks the sale.",
  "honest-sale": "One transaction: buyer pays, keys swap, 30% stays in escrow as an earn-out.",
  "agent-buys-agent": "An AI agent scans both agents, refuses the cheat and buys the good one.",
};

// The model's words, without markdown marks; long turns show their start and expand.
function said(text) {
  const t = String(text || "").replace(/\*\*|`/g, "").trim();
  if (t.length <= 260) return el("p", { class: "said", text: t });
  return el("details", { class: "said more" }, el("summary", { text: `${t.slice(0, 240).trimEnd()}… (more)` }), el("p", { text: t }));
}

const fold = (label, ...body) => el("details", { class: "fold" }, el("summary", { text: label }), ...body);

function renderNav() {
  const ol = document.getElementById("steps");
  ol.replaceChildren(...demo.steps.map((s, i) =>
    el("li", {}, el("a", { href: `#${s.id}`, class: i < current ? "done" : null, "aria-current": i === current ? "step" : null },
      el("span", { class: "n", text: String(i + 1) }), el("span", { class: "t", text: s.title })))));
  document.getElementById("bar").style.width = `${((current + 1) / demo.steps.length) * 100}%`;
  document.getElementById("count").textContent = `${current + 1} / ${demo.steps.length}`;
}

function callSummary(calls) {
  const n = (k) => calls.filter((c) => (c.display || c.outcome) === k).length;
  const parts = [["refused", n("error")], ["frozen", n("frozen")], ["unverified", n("unverified")], ["ok", n("ok")]].filter(([, v]) => v).map(([k, v]) => `${v} ${k}`);
  return `Contract calls: ${parts.join(", ")}`;
}

function render() {
  const s = demo.steps[current];
  const sec = document.getElementById("step");
  sec.dataset.step = s.id;
  const stats = el("div", { class: "stats" }, s.headline.map((h) =>
    el("div", { class: "stat" }, el("div", { class: "v", text: h.value === "true" ? "Yes" : h.value === "false" ? "No" : h.value }), el("div", { class: "l", text: h.label }))));
  const kids = [
    el("div", { class: "panel-head" }, el("span", { class: "big-n", text: String(current + 1) }), el("h2", { text: s.title })),
    el("p", { class: "short", text: SHORT[s.id] || "" }),
    stats,
  ];
  if (s.slot) {
    const slot = el("div", { class: "slot", "data-slot": "ai-buyer-transcript" }, el("h3", { text: `${s.slot.title} (${s.slot.status})` }), el("p", { class: "muted small", text: s.slot.text }));
    if (s.slot.steps && s.slot.steps.length) {
      slot.append(el("ol", { class: "chat" }, s.slot.steps.map((st) => el("li", {},
        said(st.said),
        el("div", { class: "tools" }, st.calls.map((c) => el("span", { class: "tool" },
          el("code", { text: `${c.tool}(${JSON.stringify(c.args)})` }), ` → ${c.outcome}`)))))));
    }
    kids.push(slot);
  }
  const tn = testnetBlock(s.id);
  if (tn) kids.push(tn);
  const proof = [];
  proof.push(fold("Read the full story", el("p", { class: "story", text: s.story }), el("p", { class: "ids", text: `Tests behind this step: ${s.scenarios.join(", ")}` })));
  if (s.calls && s.calls.length) proof.push(fold(callSummary(s.calls), el("ol", { class: "calls" }, s.calls.map(callCard))));
  if (s.scan) proof.push(fold(`Buyer's scan: ${s.scan.verdict}, ${s.scan.findings.length} findings`, scanBlock(s.scan)));
  if (s.listing) proof.push(fold("The listing bob's scan saw", jsonBlock("get_listing()", s.listing)));
  if (s.mcp_steps) proof.push(fold(`MCP tool calls by the summarizer agent (${s.mcp_steps.length})`, el("ol", { class: "calls" }, s.mcp_steps.map(mcpCard))));
  if (s.view) proof.push(fold(`View: ${s.view.name}`, jsonBlock(s.view.name, s.view.value)));
  kids.push(el("div", { class: "proof" }, el("h3", { text: "Proof" }), ...proof));
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
      el("a", { class: "pill link", href: "?network=testnet", text: "Check NEAR testnet" }));
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
  addEventListener("hashchange", () => { openReplayFor(location.hash); go(fromHash(), false); });
  addEventListener("keydown", (e) => {
    if (!document.getElementById("replay").open) return; // the arrows only page the replay while it is open
    if (e.target.closest && e.target.closest("input, textarea, summary, button")) return;
    if (e.key === "ArrowRight") go(current + 1);
    if (e.key === "ArrowLeft") go(current - 1);
  });
  openReplayFor(location.hash);
  go(fromHash(), false);
}

// The offline replay is folded on a plain load. A step id in the hash
// (#break-in, #cheat, ...) or testnet mode opens it; #run-cheat does not.
function openReplayFor(hash) {
  const d = document.getElementById("replay");
  if (!d || d.open) return;
  const isStep = demo && demo.steps.some((s) => `#${s.id}` === hash);
  if (!isStep && network !== "testnet") return;
  d.open = true;
  if (isStep) requestAnimationFrame(() => d.scrollIntoView());
}

// Hero proof strip: the sale tx and the old seller's refused break-in calls,
// straight from the recorded testnet run (data/testnet.json).
async function renderProofStrip() {
  const box = document.getElementById("proof-strip");
  if (!box) return;
  const t = await (await fetch("data/testnet.json")).json();
  if (t.schema !== "web-testnet.v1") return;
  const sale = (t.steps["honest-sale"]?.txs || []).find((x) => x.method === "ft_transfer_call" && x.ok);
  const tries = (t.steps["break-in"]?.txs || []).filter((x) => !x.ok && x.explorer);
  if (!sale || !tries.length) return;
  const short = (h) => `${h.slice(0, 6)}…`;
  const link = (x, text) => el("a", { href: x.explorer, target: "_blank", rel: "noopener", text });
  // Business first: three numbers, each with its transaction.
  const inv = t.steps["invest"]?.txs || [];
  const jobs = inv.filter((x) => x.ok && /pays .* for a translation/.test(x.label || ""));
  const payout = investorPayout(t);
  const stat = (num, text, x, more) => el("li", {},
    el("b", { class: "ps-num", text: num }), " ",
    el("span", { class: "ps-label", text }), " ",
    el("span", { class: "ps-links" }, x ? link(x, "tx ↗") : null, more || null));
  const stats = [];
  if (jobs.length) stats.push(stat(`${jobs.length}/${jobs.length}`, "payments in the first investment signed by NEAR AI", jobs[0]));
  if (payout) stats.push(stat(fmt(payout.investor), `of ${fmt(payout.total)} paid to the investor (${payout.pct}%)`, payout.claim));
  stats.push(stat(`${tries.length}/${tries.length}`, "ways back in refused after a sale", tries[0]));
  const after = document.getElementById("story-after");
  if (after) after.textContent = `All ${tries.length} of her ways back refused.`;
  box.replaceChildren(
    el("ul", { class: "ps-stats" }, stats),
    el("details", { class: "ps-more", id: "refused" },
      el("summary", { text: `See the ${tries.length} refused calls` }),
      el("p", { class: "ps-head" }, "On NEAR testnet: ", el("b", { text: "bob bought the agent in one transaction" }), " (", link(sale, short(sale.tx)), "). ",
        "Then the old seller tried ", el("b", { text: `${tries.length} ways back in. All refused:` })),
      el("ul", { class: "ps-list" }, tries.map((x) => el("li", {},
        el("span", { class: "ps-x", text: "✗" }), " ",
        el("span", { text: x.label.replace(/^alice: /, "") }), " ",
        el("code", { text: x.error || "refused" }), " ", link(x, short(x.tx))))),
      el("p", { class: "small muted" }, "Her old keys were deleted in the sale, so they fail at the node too. ", el("a", { href: "#evidence-run", text: "Re-check every step on chain" }), ".")));
  openTarget();
}
renderProofStrip().catch(() => { /* the static fallback text stays */ });

// "How it earns" in the Exchange header: where the money of the two recorded
// deals went (translator sold for 1,000.00; dave bought 20% for 500.00).
function renderWaterfall() {
  const box = document.getElementById("waterfall");
  if (!box) return;
  const deal = (title, w, heldLabel) => {
    const parts = [["seller now", w.seller, "wf-seller"], ["fee", w.fee, "wf-fee"], [heldLabel, w.held, "wf-held"]];
    const bar = el("div", { class: "wf-bar", role: "img", "aria-label": `${title}: ${parts.map(([k, v]) => `${fmt(v)} ${k}`).join(", ")}` },
      parts.map(([, v, c]) => { const s = el("span", { class: c }); s.style.setProperty("width", `${(100 * v) / w.total}%`); return s; }));
    return el("div", { class: "wf-deal", "data-total": String(w.total) },
      el("p", { class: "wf-title" }, el("b", { text: title }), ` ${fmt(w.total)}`), bar,
      el("p", { class: "wf-legend small" }, parts.map(([k, v, c]) => el("span", { class: `wf-key ${c}`, "data-cents": String(v) }, el("i"), `${fmt(v)} ${k}`))));
  };
  box.replaceChildren(deal("Sale", waterfall("sale", 100000), "earn-out"), deal("Share buy", waterfall("share", 50000), "escrow"));
}
renderWaterfall();

// The judge's command block: one click copies it.
document.querySelectorAll("[data-copy]").forEach((b) => b.addEventListener("click", async () => {
  const text = document.getElementById(b.dataset.copy).textContent;
  try { await navigator.clipboard.writeText(text); b.textContent = "Copied"; } catch { b.textContent = "Select and copy"; }
}));

// Due-diligence cards (scripts/diligence-data.mjs, from the recorded testnet run):
// verdict first, four numbers, a bar, and every original row under "Full due diligence".
async function renderDiligence() {
  const box = document.getElementById("dd-cards");
  if (!box) return;
  const d = await (await fetch("data/diligence.json")).json();
  if (d.schema !== "diligence.v1") return;
  const deed = document.getElementById("deed-figure");
  box.replaceChildren(...d.cards.map((c) => {
    const total = (c.bar?.real || 0) + (c.bar?.self || 0);
    const real = el("span", { class: "dd-real" });
    const self = el("span", { class: "dd-self" });
    if (total) { real.style.setProperty("width", `${(100 * c.bar.real) / total}%`); self.style.setProperty("width", `${(100 * c.bar.self) / total}%`); }
    const full = el("details", { class: "dd-full" }, el("summary", { text: "Full due diligence" }),
      c.name === "cheat" ? el("p", { class: "small dd-k8" }, el("b", { text: "New rule: " }), "the contract also refuses to list any agent that ever had a key. Proven in our offline tests; coming to testnet next.") : null,
      el("dl", {}, c.rows.flatMap(([k, v]) => [el("dt", { text: k }), el("dd", { text: v })])),
      el("p", { class: "dd-verdict" }, el("b", { text: c.verdict }), ": ", el("span", { text: c.why })));
    if (c.name === "translator" && deed) { deed.hidden = false; full.append(deed); }
    return el("article", { class: `dd-card card ${c.ok ? "dd-ok" : "dd-no"}`, "data-agent": c.name, id: `case-${c.name}` },
      el("header", { class: "dd-head" }, el("div", {}, c.role ? el("p", { class: "dd-role", text: c.role }) : null, el("h3", {}, TITLES[c.name] ? TITLES[c.name] : el("code", { text: c.name }), TITLES[c.name] ? el("small", { class: "ex-title" }, " ", el("code", { text: c.name })) : null)),
        el("span", { class: `pill ${c.ok ? "pass" : "block"}`, text: c.ok ? "Pass" : "Blocked" })),
      el("p", { class: "dd-deal small muted" }, c.deal_link ? el("a", { href: c.deal_link, text: c.deal_short || c.deal }) : (c.deal_short || c.deal)),
      el("p", { class: "dd-why", text: (c.short || c.verdict).replace(/^(Pass|Blocked): (.)/, (_, _v, ch) => ch.toUpperCase()) }),
      c.metrics ? el("dl", { class: "dd-metrics" }, c.metrics.flatMap(([k, v]) => [el("dt", { text: k }), el("dd", { text: v })])) : null,
      total ? el("div", { class: "dd-bar", role: "img", "aria-label": `real revenue ${c.bar.real}, self-paid ${c.bar.self}` }, real, self) : null,
      el("div", { class: "dd-foot" }, full,
        el("p", { class: "small dd-run" }, el("a", { href: c.run, text: "Re-check on chain" }))));
  }));
}
renderDiligence().catch(() => { /* the loading text stays */ });

// The Exchange board (scene 13): factory-born agents, sortable. Hidden until
// recorded. Multiple = asking price, or the value an open offer implies
// (price / share), over counted revenue.
async function renderExchange() {
  const box = document.getElementById("exchange");
  if (!box) return;
  const t = await (await fetch("data/testnet.json")).json();
  const x = t.exchange;
  if (!x || !Array.isArray(x.agents) || !x.agents.length) return;
  const usd = (raw) => Number(BigInt(raw)) / 1e6;
  const money = (n) => n.toLocaleString("en-US", { minimumFractionDigits: 2, maximumFractionDigits: 2 });
  const rows = x.agents.map((a) => {
    const counted = usd(a.counted);
    const gross = usd(a.gross);
    const open = a.offers.find((o) => o.sold_bps < o.bps);
    const value = a.ask ? usd(a.ask) : open ? usd(open.price) * 10000 / open.bps : null;
    const deal = a.ask ? `for sale at ${money(usd(a.ask))}` : open ? `${open.bps / 100}% of earnings for ${money(usd(open.price))}` : "not offered";
    return { ...a, counted, flaggedPct: gross ? (100 * usd(a.flagged)) / gross : 0, deal, multiple: value !== null && counted > 0 ? value / counted : Infinity };
  });
  // One cast: the three case-file agents (translator, cheat, translator3) are on the board too,
  // from the same recorded run (data/diligence.json); translator3 carries the open offer.
  const dd = await fetch("data/diligence.json").then((r) => r.json()).catch(() => null);
  const o = t.open_offer;
  for (const c of (dd && dd.cards) || []) {
    if (!c.bar) continue;
    // The open-offer agent shows its earnings when the offer was bought into
    // (data/testnet.json open_offer.earnings), the same numbers as the market page.
    const live = o && o.agent === c.agent && o.earnings ? o.earnings : null;
    const counted = live ? Number(live.counted) : c.bar.real, gross = live ? Number(live.gross) : c.bar.real + c.bar.self;
    const selfPaid = live ? Number(live.flagged) : c.bar.self;
    const payers = Number((/from (\d+) paying/.exec((c.rows.find(([k]) => /^Counted/.test(k)) || [])[1] || "") || [])[1] || 0);
    const ask = Number((/([\d.]+)$/.exec(c.deal || "") || [])[1] || 0);
    let deal = c.deal_short || c.deal, value = ask || null, target = `#case-${c.name}`, featured = false, early = null;
    if (o && o.agent === c.agent) {
      deal = `${Number(o.part_bps) / 100}% parts: ${money(usd(o.amount))} each`;
      value = usd(o.offer.price) * 10000 / Number(o.offer.bps);
      target = "#invest-yourself";
      featured = true;
      early = offerNote(o, c);
    } else if (c.name === "translator") {
      deal = `sold to bob for ${money(ask)}`;
    }
    rows.push({ name: c.name, counted, payers, deal, target, featured, early, flaggedPct: gross ? (100 * selfPaid) / gross : 0,
      multiple: value && counted > 0 ? value / counted : Infinity });
  }
  // Featured first (the agent you can invest in now), then by price over revenue.
  let key = null, dir = 1;
  const tbody = box.querySelector("tbody");
  const cmp = (k) => (a, b) => (a[k] > b[k] ? dir : a[k] < b[k] ? -dir : 0);
  const draw = () => {
    if (key) rows.sort(cmp(key)); else rows.sort((a, b) => (Number(!!b.featured) - Number(!!a.featured)) || cmp("multiple")(a, b));
    const flag = (r) => r.flaggedPct >= 50;
    tbody.replaceChildren(...rows.map((r) => el("tr", { class: [flag(r) ? "ex-caught" : "", r.featured ? "ex-featured" : "", r.target ? "ex-link" : ""].join(" ").trim() || null, "data-target": r.target },
      el("td", { "data-label": "Agent", class: "ex-name" }, r.target ? el("a", { href: r.target }, TITLES[r.name] || el("code", { text: r.name })) : (TITLES[r.name] || el("code", { text: r.name })),
        TITLES[r.name] ? el("small", { class: "ex-title" }, " ", el("code", { text: r.name })) : null, " ",
        flag(r) ? el("span", { class: "pill block", text: `caught: ${r.flaggedPct.toFixed(0)}% self-paid` })
          : r.featured ? el("span", { class: "pill pass", text: "open to invest" }) : null),
      el("td", { "data-label": "Real revenue", class: "num", text: r.counted.toFixed(2) }),
      el("td", { "data-label": "Self-paid", class: `num${flag(r) ? " bad" : ""}`, text: `${r.flaggedPct.toFixed(0)}%` }),
      el("td", { "data-label": "Customers", class: "num", text: String(r.payers) }),
      el("td", { "data-label": "Deal", class: "ex-deal", text: r.deal }),
      el("td", { "data-label": "Price ÷ revenue", class: "num", title: r.early ? r.early.text : null },
        flag(r) ? "price unsupported" : Number.isFinite(r.multiple) ? `${r.multiple.toFixed(1)}×` : "no real revenue",
        r.early ? el("small", { class: "ex-early", text: r.early.short }) : null))));
    box.querySelectorAll("th[data-k]").forEach((th) => {
      if (th.dataset.k === key) th.setAttribute("aria-sort", dir > 0 ? "ascending" : "descending"); else th.removeAttribute("aria-sort");
    });
  };
  // A whole row is a link to its case file (or the invest card): the name is the real link.
  tbody.addEventListener("click", (e) => {
    const tr = e.target.closest("tr[data-target]");
    if (!tr || e.target.closest("a")) return;
    location.hash = tr.dataset.target;
  });
  box.querySelectorAll("th[data-k]").forEach((th) => {
    th.tabIndex = 0;
    const sort = () => { dir = key === th.dataset.k ? -dir : 1; key = th.dataset.k; draw(); };
    th.addEventListener("click", sort);
    th.addEventListener("keydown", (e) => { if (e.key === "Enter" || e.key === " ") { e.preventDefault(); sort(); } });
  });
  draw();
  document.getElementById("ex-note").textContent = `${x.agents.map((a) => a.name).join(", ")}: born with no key. Recorded on testnet.`;
  box.hidden = false;
}
renderExchange().catch(() => { /* the board stays hidden */ });

// The early-multiple note for the open offer, from the recorded data: the
// days with counted revenue (diligence card) and the offer's own terms.
function offerNote(o, card) {
  if (!o || !card) return null;
  const cents = (raw) => Math.round(Number(BigInt(raw)) / 1e4);
  return earlyNote({ days: Number((o.earnings && o.earnings.days) || card.revenue_days || 0), counted: Math.round((o.earnings ? Number(o.earnings.counted) : card.bar.real) * 100), price: cents(o.offer.price), bps: Number(o.offer.bps), end: Number(BigInt(o.offer.end) / 1000000n) });
}

// "Invest yourself": the open offer recorded by scene 12. Hidden until the
// offer exists in data/testnet.json. The wallet returns here with the tx hash,
// which is shown as a receipt and kept in this browser (localStorage).
// The market page's product names (web/market.js), next to the account names.
const TITLES = { translator3: "Lingo Live", "translate-pro": "Translate Pro", "research-desk": "Research Desk", "pump-bot": "TrendPulse", cheat: "QuickQuote", translator: "Polyglot" };
const NB = "https://testnet.nearblocks.io";
const MY_KEY = "ae.myInvestments";
function loadMine() {
  try { const v = JSON.parse(localStorage.getItem(MY_KEY) || "[]"); return Array.isArray(v) ? v.filter((x) => x && typeof x.account === "string" && typeof x.tx === "string") : []; } catch { return []; }
}
function saveMine(list) {
  try { localStorage.setItem(MY_KEY, JSON.stringify(list.slice(0, 20))); } catch { /* storage blocked: the receipt still shows now */ }
}
const ago = (t) => {
  const s = Math.max(0, (Date.now() - Number(t)) / 1000);
  return s < 120 ? "just now" : s < 7200 ? `${Math.round(s / 60)} min ago` : s < 172800 ? `${Math.round(s / 3600)} h ago` : `${Math.round(s / 86400)} days ago`;
};

async function renderInvestYourself() {
  const box = document.getElementById("invest-yourself");
  if (!box) return;
  const t = await (await fetch("data/testnet.json")).json();
  const o = t.open_offer;
  if (!o || !o.agent || !o.offer) return;
  const usd = (raw) => (Number(BigInt(raw)) / 1e6).toFixed(2);
  const pct = Number(o.part_bps) / 100;
  const endDate = new Date(Number(BigInt(o.offer.end) / 1000000n));
  const end = endDate.toISOString().slice(0, 10);
  const endText = endDate.toLocaleDateString("en-GB", { day: "numeric", month: "short", year: "numeric", timeZone: "UTC" });
  const acct = o.agent.split(".")[0];
  const name = TITLES[acct] || acct; // the brand name people read; the account id stays in the tooltip
  const title = document.getElementById("iy-title");
  const dollars = Math.round(Number(usd(o.amount)));
  title.textContent = `Invest ${pct}% of ${name}'s earnings (${dollars} test $)`;
  title.title = o.agent;
  // One offer, one sentence: the pool, your part of it, what is sold. Same unit (%) as the buttons.
  const pool = Number(o.offer.bps) / 100;
  const parts = Number(o.offer.bps) / Number(o.part_bps);
  const sold = Number(o.offer.sold_bps) / Number(o.part_bps);
  document.getElementById("iy-sum").textContent =
    `${name} shares ${pool}% of its earnings with investors. One part = ${pct}% of every customer payment. ${sold} of ${parts} parts sold.`;
  const term = (k, v) => el("div", {}, el("dt", { text: k }), el("dd", { text: v }));
  document.getElementById("iy-terms").replaceChildren(
    term("You get", `${pct}% of every customer payment`),
    term("You pay", `${usd(o.amount)} test dollars`),
    term("Until", endText));
  const dd = await fetch("data/diligence.json").then((r) => r.json()).catch(() => null);
  const note = offerNote(o, dd && dd.cards.find((c) => c.agent === o.agent));
  const early = document.getElementById("iy-early");
  if (note) { early.textContent = note.text; early.hidden = false; }
  if (o.example_tx && o.example_tx.explorer) {
    document.getElementById("iy-example").replaceChildren(" · ",
      el("a", { href: o.example_tx.explorer, target: "_blank", rel: "noopener noreferrer", text: "see a recorded investment ↗" }));
  }
  const call = (to, method, args, gas, dep) => `near contract call-function as-transaction ${to} ${method} json-args '${JSON.stringify(args)}' prepaid-gas '${gas} Tgas' attached-deposit '${dep}' sign-as YOU.testnet network-config testnet sign-with-keychain send`;
  const view = (to, method, args) => `near contract call-function as-read-only ${to} ${method} json-args '${JSON.stringify(args)}' network-config testnet now`;
  const steps = [
    ["Take free test dollars (the faucet registers you)", [call(o.xusd, "faucet", { amount: o.amount }, 30, "0 NEAR")]],
    ["Check the agent before paying: its earnings by who paid, and the open offer", [view(o.agent, "get_earnings", {}), view(o.agent, "get_shares", {})]],
    [`Buy a ${pct}% part: one transfer carries the offer's exact terms (until ${end})`, [call(o.xusd, "ft_transfer_call", { receiver_id: o.agent, amount: o.amount, msg: o.msg }, 150, "1 yoctoNEAR")]],
    ["See your holding; each customer payment adds to what you can claim", [view(o.agent, "get_holdings", { account_id: "YOU.testnet" })]],
    ["Claim your part of the earnings", [call(o.agent, "claim", {}, 50, "0 NEAR")]],
  ];
  document.getElementById("iy-cli-note").replaceChildren("Agent ", el("code", { text: o.agent }), ". Replace ", el("code", { text: "YOU.testnet" }), " with your account.");
  document.getElementById("iy-steps").replaceChildren(...steps.map(([what, cmds]) =>
    el("li", {}, el("p", { text: what }), ...cmds.map((c) => el("pre", {}, el("code", { text: c }))))));
  box.hidden = false;
  document.getElementById("iy-invest").textContent = `Invest ${pct}% with MyNearWallet`;
  renderInvestors(o, null);
  document.getElementById("iy-refresh").addEventListener("click", () => {
    document.getElementById("iy-live").textContent = "Reading the offer on testnet…";
    liveOffer(o).catch((e) => { document.getElementById("iy-live").textContent = `Live read failed: ${e.message}`; });
  });
  wireWallet(o, { pct, name });
  openTarget();
}

// "Investors so far": this browser's own investments first, then the holders
// (recorded in data/testnet.json, or live after "Refresh live").
function renderInvestors(o, liveHolders) {
  const ul = document.getElementById("iy-investors");
  const mine = loadMine().filter((m) => m.agent === o.agent);
  const mineAcc = new Set(mine.map((m) => m.account));
  // Recorded holders carry a label (the MCP agent investor; the page's Invest tx signed by script).
  const known = new Map(investorList(o).map((h) => [h.account, h]));
  const base = liveHolders ? liveHolders.map((h) => ({ ...known.get(h.account), ...h })) : [...known.values()];
  for (const h of known.values()) if (!base.some((b) => b.account === h.account)) base.push(h);
  const holders = base.filter((h) => !mineAcc.has(h.account));
  ul.replaceChildren(
    ...mine.map((m) => el("li", { class: "mine" },
      el("b", { text: m.account }), ` · ${m.pct}% · `,
      el("a", { href: `${NB}/txns/${m.tx}`, target: "_blank", rel: "noopener", text: "tx ↗" }), ` · ${ago(m.at)}`)),
    ...holders.map((h) => el("li", {},
      el("a", { href: `${NB}/address/${h.account}`, target: "_blank", rel: "noopener", text: h.account }),
      ` · ${Number(h.bps) / 100}%`,
      h.label ? ` · ${h.label}` : "",
      h.tx ? [" · ", el("a", { href: `${NB}/txns/${h.tx}`, target: "_blank", rel: "noopener", text: "tx ↗" })] : "",
      ` · ${liveHolders ? "live" : "recorded"}`)));
}

// One-click links to MyNearWallet's sign page (web/wallet-link.mjs). The wallet
// returns to ?iy=<account>&transactionHashes=… (or errorCode=… if cancelled).
function wireWallet(o, { pct, name }) {
  const input = document.getElementById("iy-account");
  const invest = document.getElementById("iy-invest");
  const claim = document.getElementById("iy-claim");
  const read = document.getElementById("iy-read");
  const out = document.getElementById("iy-out");
  const after = document.querySelector("#invest-yourself .iy-after");
  const receipt = document.getElementById("iy-receipt");
  const back = (who) => { const u = new URL(location.href); u.search = ""; u.hash = ""; u.searchParams.set("iy", who); return u.toString(); };
  // typed: the visitor changed the box, so the status line may be rewritten.
  const update = (typed) => {
    const who = input.value.trim().toLowerCase();
    const ok = validAccount(who) && who.endsWith(".testnet");
    for (const a of [invest, claim]) {
      if (ok) a.removeAttribute("aria-disabled"); else { a.setAttribute("aria-disabled", "true"); a.removeAttribute("href"); }
    }
    read.disabled = !ok;
    after.hidden = !ok;
    if (ok) input.removeAttribute("aria-invalid");
    if (!ok) {
      if (!typed) return;
      // Say why nothing happens: a .near name, or not an account name yet.
      if (who.endsWith(".near")) out.textContent = `${who} is a real-money .near account. This demo runs on NEAR testnet: create a free account ending in .testnet (link below), then type it here.`;
      else if (who.includes(".") && !who.endsWith(".testnet")) out.textContent = "Use a testnet account: its name ends in .testnet, e.g. you.testnet.";
      else if (who && !validAccount(who)) out.textContent = "Account names use a-z, 0-9, - and _ and end in .testnet.";
      else out.textContent = "Sign once. Free test money.";
      return;
    }
    if (typed) out.textContent = "Ready: the button opens MyNearWallet to sign one transaction.";
    invest.href = signUrl([investTx(o, who)], back(who));
    claim.href = signUrl([claimTx(o, who)], back(who));
  };
  input.addEventListener("input", () => update(true));
  // The button keeps its colour; with no valid account a click says what to do.
  invest.addEventListener("click", (e) => {
    if (invest.getAttribute("aria-disabled") !== "true") return;
    e.preventDefault();
    input.setAttribute("aria-invalid", "true");
    input.focus();
    invest.classList.remove("shake");
    void invest.offsetWidth; // restart the animation
    invest.classList.add("shake");
    out.textContent = "Type your testnet account first, e.g. you.testnet.";
  });
  claim.addEventListener("click", (e) => { if (claim.getAttribute("aria-disabled") === "true") e.preventDefault(); });
  const holding = async (who) => {
    const res = await rpcQuery({ request_type: "call_function", account_id: o.agent, method_name: "get_holdings", args_base64: btoa(JSON.stringify({ account_id: who })) });
    const h = JSON.parse(new TextDecoder().decode(new Uint8Array(res.result)));
    return { bps: (h.holdings || []).reduce((n, x) => n + Number(x.bps), 0), claimable: (Number(h.claimable) / 1e6).toFixed(2) };
  };
  read.addEventListener("click", async () => {
    const who = input.value.trim().toLowerCase();
    out.textContent = "Reading the agent's ledger on testnet…";
    try {
      const h = await holding(who);
      liveOffer(o).catch(() => { /* the recorded terms stay */ });
      out.textContent = h.bps ? `${who} holds ${h.bps / 100}% of ${name}'s earnings offer; claimable now: ${h.claimable} test dollars.` : `${who} holds no part of ${name} yet.`;
    } catch (e) {
      out.textContent = `Live read failed (it needs the page's local server, make web): ${e.message}`;
    }
  });
  const who = params.get("iy");
  if (who && validAccount(who)) {
    input.value = who;
    const hashes = (params.get("transactionHashes") || "").split(",").filter((h) => /^[1-9A-HJ-NP-Za-km-z]{43,44}$/.test(h));
    if (hashes.length) {
      const h = hashes[hashes.length - 1];
      const earn = el("span", { class: "iy-earn", text: "reading…" });
      receipt.replaceChildren(
        el("p", { class: "iy-ok" }, el("b", { text: `✓ Invested. You own ${pct}% of ${name}.` })),
        el("p", { class: "small" }, el("a", { href: `${NB}/txns/${h}`, target: "_blank", rel: "noopener", text: `tx ${h.slice(0, 8)}… ↗` }),
          " · Your earnings: ", earn, " (live)"));
      receipt.hidden = false;
      out.textContent = "Signed and sent from MyNearWallet.";
      const mine = loadMine().filter((m) => m.tx !== h);
      mine.unshift({ account: who, tx: h, pct, agent: o.agent, at: Date.now() });
      saveMine(mine);
      renderInvestors(o, null);
      holding(who).then((x) => { earn.textContent = `${x.claimable} test dollars claimable`; }).catch(() => { earn.textContent = "not readable from here"; });
      liveOffer(o).catch(() => { /* the recorded list stays */ });
    } else if (params.get("errorCode")) {
      out.textContent = "The wallet did not send it. Nothing changed.";
      receipt.hidden = true;
    }
    document.getElementById("invest-yourself").scrollIntoView();
  }
  update();
}

// Live count of parts sold in the open offer (read-only relay; on "Refresh live"
// or on return from the wallet, never on a plain load).
async function liveOffer(o) {
  const res = await rpcQuery({ request_type: "call_function", account_id: o.agent, method_name: "get_shares", args_base64: btoa("{}") });
  const offers = JSON.parse(new TextDecoder().decode(new Uint8Array(res.result)));
  const live = offers.find((x) => String(x.offer_id) === String(o.offer.offer_id));
  if (!live) { document.getElementById("iy-live").textContent = "The offer is no longer open on testnet."; return; }
  const parts = Number(live.bps) / Number(o.part_bps);
  const sold = Number(live.sold_bps) / Number(o.part_bps);
  const who = new Set((live.holders || []).map((h) => h.account)).size;
  document.getElementById("iy-live").textContent = `Live on testnet now: ${sold} of ${parts} parts sold, to ${who} investor${who === 1 ? "" : "s"}.`;
  // In the live list, keep this browser's own entries on top (they carry the tx).
  renderInvestors(o, live.holders || []);
}
renderInvestYourself().catch(() => { /* the section stays hidden */ });

main().catch((e) => {
  document.getElementById("step").replaceChildren(el("p", { class: "err", text: `Could not load the demo: ${e.message}` }));
});

// A link into a folded section opens it (e.g. #backend inside "Technical details"),
// and every closed fold around it.
function openTarget() {
  const id = location.hash.slice(1);
  const t = id && document.getElementById(id);
  if (!t) return;
  let opened = false;
  for (let d = t.closest("details"); d; d = d.parentElement && d.parentElement.closest("details")) {
    if (!d.open) { d.open = true; opened = true; }
  }
  if (opened) t.scrollIntoView();
}

// "Scan any agent": the example buttons fill the account and submit.
document.querySelectorAll("[data-example]").forEach((b) => b.addEventListener("click", () => {
  const form = document.getElementById("check-form");
  document.getElementById("check-account").value = b.dataset.example;
  if (form.requestSubmit) form.requestSubmit(); else form.querySelector("button").click();
}));
addEventListener("hashchange", openTarget);
openTarget();
