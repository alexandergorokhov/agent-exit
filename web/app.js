// Agent Exit demo page. Offline by default: every result shown is read from
// data/demo.json, which tools/verify/src/bin/gen-web-demo.rs records from the
// contract in near-sdk's mocked runtime. `?network=testnet` shows the testnet
// mode: the recorded testnet run of 2026-11-11 (agentexit5.testnet) with explorer links, plus read-only live reads.

// Vendored and hash-pinned (web/vendor/SHA256SUMS); the page CSP allows no remote script.
const NEAR_CONNECT = "./vendor/near-connect-0.11.4.esm.js";
import { signUrl, investTx, claimTx, validAccount } from "./wallet-link.mjs";
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
      el("a", { class: "pill link", href: "?network=testnet", text: "Check NEAR testnet →" }));
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
  addEventListener("keydown", (e) => {
    if (e.target.closest && e.target.closest("input, textarea, summary, button")) return;
    if (e.key === "ArrowRight") go(current + 1);
    if (e.key === "ArrowLeft") go(current - 1);
  });
  go(fromHash(), false);
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
  const share = inv.find((x) => x.ok && x.method === "ft_transfer_call" && /buys the 20% slice/.test(x.label || ""));
  const claim = inv.find((x) => x.ok && /claim/i.test(x.label || ""));
  const stat = (num, text, x) => el("li", {}, el("b", { class: "ps-num", text: num }), " ", el("span", { text }), x ? [" ", link(x, short(x.tx))] : []);
  const stats = [];
  if (jobs.length) stats.push(stat(`${jobs.length}/${jobs.length}`, "customer payments signed by NEAR AI and checked on-chain", jobs[0]));
  if (share && claim) stats.push(stat("20%", "of earnings bought by an investor, who got exactly 20%", claim));
  stats.push(stat(`${tries.length}/${tries.length}`, "break-in attempts by the old seller refused after the sale", sale));
  box.replaceChildren(
    el("ul", { class: "ps-stats" }, stats),
    el("p", { class: "ps-head" }, "On NEAR testnet: ", el("b", { text: "bob bought the agent in one transaction" }), " (", link(sale, short(sale.tx)), "). ",
      "Then the old seller tried to take it back ", el("b", { text: `${tries.length} times. All refused:` })),
    el("ul", { class: "ps-list" }, tries.map((x) => el("li", {},
      el("span", { class: "ps-x", text: "✗" }), " ",
      el("span", { text: x.label.replace(/^alice: /, "") }), " ",
      el("code", { text: x.error || "refused" }), " ", link(x, short(x.tx))))),
    el("p", { class: "small muted" }, "Her old keys were deleted in the sale, so they fail at the node too. ", el("a", { href: "#evidence-run", text: "Re-check every step on chain" }), "."));
}
renderProofStrip().catch(() => { /* the static fallback text stays */ });

// Due-diligence cards (scripts/diligence-data.mjs, from the recorded testnet run).
async function renderDiligence() {
  const box = document.getElementById("dd-cards");
  if (!box) return;
  const d = await (await fetch("data/diligence.json")).json();
  if (d.schema !== "diligence.v1") return;
  box.replaceChildren(...d.cards.map((c) => el("article", { class: `dd-card ${c.ok ? "dd-ok" : "dd-no"}`, "data-agent": c.name },
    el("h3", {}, el("code", { text: c.name }), " ", el("span", { class: "muted small", text: c.deal })),
    el("dl", {}, c.rows.flatMap(([k, v]) => [el("dt", { text: k }), el("dd", { text: v })])),
    el("p", { class: "dd-verdict" }, el("b", { text: c.verdict }), ": ", el("span", { text: c.why })),
    el("p", { class: "small" }, el("a", { href: c.run, text: "Re-check it on NEAR testnet now" })))));
  box.after(el("p", { class: "small muted", text: d.note }));
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
  const rows = x.agents.map((a) => {
    const counted = usd(a.counted);
    const gross = usd(a.gross);
    const open = a.offers.find((o) => o.sold_bps < o.bps);
    const value = a.ask ? usd(a.ask) : open ? usd(open.price) * 10000 / open.bps : null;
    const deal = a.ask ? `for sale at ${usd(a.ask).toFixed(2)}` : open ? `${open.bps / 100}% of earnings for ${usd(open.price).toFixed(2)}` : "not offered";
    return { ...a, counted, flaggedPct: gross ? (100 * usd(a.flagged)) / gross : 0, deal, multiple: value !== null && counted > 0 ? value / counted : Infinity };
  });
  let key = "multiple", dir = 1;
  const tbody = box.querySelector("tbody");
  const draw = () => {
    rows.sort((a, b) => (a[key] > b[key] ? dir : a[key] < b[key] ? -dir : 0));
    tbody.replaceChildren(...rows.map((r) => el("tr", { class: r.flaggedPct >= 50 ? "ex-flag" : "" },
      el("td", {}, el("code", { text: r.name }), r.born_clean ? el("span", { class: "badge ok", text: "born clean" }) : null),
      el("td", { text: r.counted.toFixed(2) }),
      el("td", { text: `${r.flaggedPct.toFixed(0)}%` }),
      el("td", { text: String(r.payers) }),
      el("td", { text: r.deal }),
      el("td", { text: Number.isFinite(r.multiple) ? `${r.multiple.toFixed(1)}×` : "no counted revenue" }))));
  };
  box.querySelectorAll("th[data-k]").forEach((th) => th.addEventListener("click", () => {
    dir = key === th.dataset.k ? -dir : 1; key = th.dataset.k; draw();
  }));
  draw();
  document.getElementById("ex-note").textContent = `Recorded on NEAR testnet from the factory ${x.factory}. All money is xUSD-FAKE. A high flagged share means the owner paid the agent herself: the meter does not count it.`;
  box.hidden = false;
}
renderExchange().catch(() => { /* the board stays hidden */ });

// "Invest yourself": the open offer recorded by scene 12, as copyable
// near-cli-rs commands. Hidden until the offer exists in data/testnet.json.
async function renderInvestYourself() {
  const box = document.getElementById("invest-yourself");
  if (!box) return;
  const t = await (await fetch("data/testnet.json")).json();
  const o = t.open_offer;
  if (!o || !o.agent || !o.offer) return;
  const usd = (raw) => (Number(BigInt(raw)) / 1e6).toFixed(2);
  const pct = Number(o.part_bps) / 100;
  const end = new Date(Number(BigInt(o.offer.end) / 1000000n)).toISOString().slice(0, 10);
  document.getElementById("iy-terms").textContent =
    `${o.agent} pays ${Number(o.offer.bps) / 100}% of every customer payment to its investors until ${end}, for ${usd(o.offer.price)} xUSD-FAKE in all. A ${pct}% part costs ${usd(o.amount)}.`;
  const call = (to, method, args, gas, dep) => `near contract call-function as-transaction ${to} ${method} json-args '${JSON.stringify(args)}' prepaid-gas '${gas} Tgas' attached-deposit '${dep}' sign-as YOU.testnet network-config testnet sign-with-keychain send`;
  const view = (to, method, args) => `near contract call-function as-read-only ${to} ${method} json-args '${JSON.stringify(args)}' network-config testnet now`;
  const steps = [
    ["Take free test dollars (the faucet registers you)", [call(o.xusd, "faucet", { amount: o.amount }, 30, "0 NEAR")]],
    ["Check the agent before paying: its earnings by who paid, and the open offer", [view(o.agent, "get_earnings", {}), view(o.agent, "get_shares", {})]],
    [`Buy a ${pct}% part: one transfer carries the offer's exact terms`, [call(o.xusd, "ft_transfer_call", { receiver_id: o.agent, amount: o.amount, msg: o.msg }, 150, "1 yoctoNEAR")]],
    ["See your holding; each customer payment adds to what you can claim", [view(o.agent, "get_holdings", { account_id: "YOU.testnet" })]],
    ["Claim your part of the earnings", [call(o.agent, "claim", {}, 50, "0 NEAR")]],
  ];
  document.getElementById("iy-steps").replaceChildren(...steps.map(([what, cmds]) =>
    el("li", {}, el("p", { text: what }), ...cmds.map((c) => el("pre", {}, el("code", { text: c }))))));
  box.hidden = false;
  document.getElementById("iy-invest").textContent = `Invest ${pct}% with MyNearWallet`;
  wireWallet(o);
}

// One-click links to MyNearWallet's sign page (web/wallet-link.mjs). The wallet
// returns to ?iy=<account>&transactionHashes=… (or errorCode=… if cancelled).
function wireWallet(o) {
  const input = document.getElementById("iy-account");
  const invest = document.getElementById("iy-invest");
  const claim = document.getElementById("iy-claim");
  const read = document.getElementById("iy-read");
  const out = document.getElementById("iy-out");
  const back = (who) => { const u = new URL(location.href); u.search = ""; u.hash = ""; u.searchParams.set("iy", who); return u.toString(); };
  const update = () => {
    const who = input.value.trim().toLowerCase();
    const ok = validAccount(who) && who.endsWith(".testnet");
    for (const a of [invest, claim]) {
      if (ok) a.removeAttribute("aria-disabled"); else { a.setAttribute("aria-disabled", "true"); a.removeAttribute("href"); }
    }
    read.disabled = !ok;
    if (!ok) return;
    invest.href = signUrl([investTx(o, who)], back(who));
    claim.href = signUrl([claimTx(o, who)], back(who));
  };
  input.addEventListener("input", update);
  read.addEventListener("click", async () => {
    const who = input.value.trim().toLowerCase();
    out.textContent = "Reading the agent's ledger on testnet…";
    try {
      const res = await rpcQuery({ request_type: "call_function", account_id: o.agent, method_name: "get_holdings", args_base64: btoa(JSON.stringify({ account_id: who })) });
      const h = JSON.parse(new TextDecoder().decode(new Uint8Array(res.result)));
      const bps = (h.holdings || []).reduce((n, x) => n + Number(x.bps), 0);
      liveOffer(o).catch(() => { /* the recorded terms stay */ });
      out.textContent = bps ? `${who} holds ${bps / 100}% of ${o.agent}'s revenue offer; claimable now: ${(Number(h.claimable) / 1e6).toFixed(2)} xUSD-FAKE.` : `${who} holds no part of ${o.agent} yet.`;
    } catch (e) {
      out.textContent = `Live read failed (it needs the page's local server, make web): ${e.message}`;
    }
  });
  const who = params.get("iy");
  if (who && validAccount(who)) {
    input.value = who;
    const hashes = (params.get("transactionHashes") || "").split(",").filter((h) => /^[1-9A-HJ-NP-Za-km-z]{43,44}$/.test(h));
    if (hashes.length) out.replaceChildren("Signed and sent: ", ...hashes.map((h) => el("a", { href: `https://testnet.nearblocks.io/txns/${h}`, target: "_blank", rel: "noopener", text: `${h.slice(0, 8)}…` })), ". Press See my holding.");
    else if (params.get("errorCode")) out.textContent = "The wallet did not send the transaction (cancelled or failed). Nothing changed.";
    document.getElementById("invest-yourself").scrollIntoView();
  }
  update();
}

// Live count of parts sold in the open offer (read-only relay, on click only; only where the page's server runs).
async function liveOffer(o) {
  const res = await rpcQuery({ request_type: "call_function", account_id: o.agent, method_name: "get_shares", args_base64: btoa("{}") });
  const offers = JSON.parse(new TextDecoder().decode(new Uint8Array(res.result)));
  const live = offers.find((x) => String(x.offer_id) === String(o.offer.offer_id));
  if (!live) return;
  const parts = Number(live.bps) / Number(o.part_bps);
  const sold = Number(live.sold_bps) / Number(o.part_bps);
  const who = new Set((live.holders || []).map((h) => h.account)).size;
  document.getElementById("iy-live").textContent = `Live on testnet now: ${sold} of ${parts} parts sold, to ${who} investor${who === 1 ? "" : "s"}.`;
}
renderInvestYourself().catch(() => { /* the section stays hidden */ });

main().catch((e) => {
  document.getElementById("step").replaceChildren(el("p", { class: "err", text: `Could not load the demo: ${e.message}` }));
});

// A link into a folded section opens it (e.g. #backend inside "How it works").
function openTarget() {
  const id = location.hash.slice(1);
  const t = id && document.getElementById(id);
  const d = t && t.closest("details");
  if (d && !d.open) { d.open = true; t.scrollIntoView(); }
}
addEventListener("hashchange", openTarget);
openTarget();
