// The market: a catalog of AI agents recorded on NEAR testnet (world in
// data/testnet.json), each with a verdict, and a detail panel with the due
// diligence and one action. Data only from data/testnet.json and
// data/diligence.json; nothing is invented here.
import { signUrl, investTx, validAccount } from "./wallet-link.mjs";

const NB = "https://testnet.nearblocks.io/txns/";

function el(tag, attrs = {}, ...kids) {
  const n = document.createElement(tag);
  for (const [k, v] of Object.entries(attrs)) {
    if (v == null || v === false) continue;
    if (k === "class") n.className = v;
    else if (k === "text") n.textContent = v;
    else if (k.startsWith("on")) n.addEventListener(k.slice(2), v);
    else n.setAttribute(k, v === true ? "" : v);
  }
  for (const k of kids.flat()) if (k != null) n.append(k.nodeType ? k : String(k));
  return n;
}
const usd = (raw) => Number(BigInt(raw)) / 1e6;
const day = (ns) => new Date(Number(BigInt(ns) / 1000000n)).toISOString().slice(0, 10);
const money = (n) => n.toLocaleString("en-US", { minimumFractionDigits: 2, maximumFractionDigits: 2 });
const link = (href, text) => el("a", { href, target: "_blank", rel: "noopener noreferrer", text });

// Product names for people; the account stays in the panel. pump-bot and
// cheat are our own ill-intended test agents, labelled as such.
const TITLES = { translator3: "Lingo Live", "translate-pro": "Translate Pro", "research-desk": "Research Desk", "pump-bot": "TrendPulse", cheat: "QuickQuote", translator: "Polyglot" };
const ILL = new Set(["pump-bot", "cheat"]);

// One shape for every agent, from the two recorded files.
export function buildCatalog(t, dd) {
  const out = [];
  const card = (name) => (dd && dd.cards || []).find((c) => c.name === name);
  // A recorded transaction by its label, shown under a plain name.
  const tx = (key, re, label) => {
    const x = ((t.steps[key] && t.steps[key].txs) || []).find((y) => re.test(y.label));
    return x ? [{ label, url: x.explorer, ok: x.ok }] : [];
  };
  const o = t.open_offer;

  const t3 = card("translator3");
  if (t3 && o) {
    const pct = Number(o.part_bps) / 100;
    const e = o.earnings; // earnings when the offer was bought into, if recorded
    const real = e ? Number(e.counted) : t3.bar.real, self = e ? Number(e.flagged) : t3.bar.self;
    const ver = e ? Number(e.verified) : null;
    const until = day(o.offer.end);
    out.push({
      name: "translator3", account: o.agent, kind: "invest", stamp: ["Verified", "ok"],
      judgement: ver == null ? "Its counted jobs are signed by NEAR AI." : `${money(ver)} of its ${money(real)} counted revenue is signed by NEAR AI.`,
      real, self, customers: 5, keys: "clean",
      deal: `${pct}% of its earnings for ${money(usd(o.amount))}, until ${until}`,
      dealShort: `${pct}% for ${money(usd(o.amount))}`,
      ends: Number(BigInt(o.offer.end) / 1000000n),
      rows: e ? [
        ["Revenue paid in", money(Number(e.gross))],
        ["Counted (real customers)", money(real)],
        ["Signed by NEAR AI", money(ver)],
        ["Self-paid, not counted", money(self)],
        ["Offer", `${Number(o.offer.bps) / 100}% of earnings in parts of ${pct}%, ${Number(o.offer.sold_bps) / 100}% sold, until ${until}`],
      ] : t3.rows, offer: o, txs: [{ label: "an investor buys 2.5%", url: o.example_tx.explorer, ok: true }, ...tx("invest", /translation$/, "a customer pays"), ...tx("invest", /claims$/, "the investor is paid")],
    });
  }
  for (const a of (t.exchange && t.exchange.agents) || []) {
    const gross = usd(a.gross), real = usd(a.counted), self = usd(a.flagged);
    const open = a.offers.find((x) => x.sold_bps < x.bps);
    const flagged = gross > 0 && self / gross >= 0.5;
    out.push({
      name: a.name, account: a.agent, kind: flagged ? "flagged" : a.ask ? "listed" : open ? "offer" : "held", ask: a.ask, open,
      stamp: flagged ? ["Self-paid", "bad"] : ["Clean", "ok"],
      judgement: flagged
        ? `${Math.round((100 * self) / gross)}% of its revenue is the owner paying itself.`
        : `${a.payers} paying customers. Born with no seller key.`,
      real, self, customers: a.payers, keys: a.born_clean ? "none, ever" : "clean",
      deal: a.ask ? `whole agent for ${money(usd(a.ask))}` : open ? `${open.bps / 100}% of its earnings for ${money(usd(open.price))}` : "not offered",
      dealShort: a.ask ? `${money(usd(a.ask))}` : open ? `${open.bps / 100}% for ${money(usd(open.price))}` : "—",
      rows: [
        ["Revenue paid in", money(gross)],
        ["Counted (real customers)", `${money(real)} from ${a.payers} paying account${a.payers === 1 ? "" : "s"}`],
        ["Self-paid, not counted", money(self)],
        ["Keys", a.born_clean ? "made by the factory with no key, ever" : "clean"],
      ],
      txs: a.name === "translate-pro" ? [{ label: "born with no key", url: `${NB}8VyRw9PPr5H2bGP4rLzFSzEKJMrDErdoAMJbkExb3Li7`, ok: true }] : [],
    });
  }
  const cheat = card("cheat");
  if (cheat) out.push({
    name: "cheat", account: cheat.agent, kind: "blocked", stamp: ["Blocked", "bad"],
    judgement: "Self-paid revenue and a hidden seller key.",
    real: cheat.bar.real, self: cheat.bar.self, customers: 0, keys: "hidden full-access key",
    deal: "listed for 500.00; every buy is refused", dealShort: "buy refused",
    rows: cheat.rows, txs: tx("cheat", /lists cheat/, "the listing"),
  });
  const tr = card("translator");
  if (tr) out.push({
    name: "translator", account: tr.agent, kind: "sold", stamp: ["Sold", "sold"],
    judgement: "Sold. The seller tried 6 ways back in; all failed.",
    real: tr.bar.real, self: tr.bar.self, customers: 6, keys: "seller's key deleted",
    deal: "sold to bob for 1,000.00", dealShort: "sold, 1,000.00",
    rows: tr.rows, txs: [...tx("honest-sale", /buys translator/, "the sale"), ...tx("break-in", /withdraw/, "seller tries to withdraw"), ...tx("break-in", /old runner key/, "seller's old key")],
  });
  for (const a of out) { a.title = TITLES[a.name] || a.name; a.ill = ILL.has(a.name); }
  return out;
}

const FILTERS = [["all", "All"], ["buy", "Open to buy"], ["flag", "Flagged"], ["sold", "Sold"]];
const match = (f, a) => f === "all" || (f === "buy" && (a.kind === "invest" || a.kind === "offer" || a.kind === "listed")) || (f === "flag" && (a.kind === "flagged" || a.kind === "blocked")) || (f === "sold" && a.kind === "sold");

function meter(a) {
  const total = a.real + a.self || 1;
  return el("div", { class: "meter", role: "img", "aria-label": `${money(a.real)} real revenue, ${money(a.self)} self-paid` },
    el("span", { class: "m-real", style: null, "data-w": (100 * a.real) / total }),
    el("span", { class: "m-self", "data-w": (100 * a.self) / total }));
}
// CSP: no inline styles from markup; widths are set through the CSSOM.
function sizeMeters(root) { root.querySelectorAll(".meter span").forEach((s) => { s.style.width = `${s.dataset.w}%`; }); }

function cardEl(a) {
  return el("button", { type: "button", class: `mk-deed k-${a.kind}`, "data-agent": a.name, "aria-haspopup": "dialog" },
    el("span", { class: "mk-deed-top" }, el("span", { class: "mk-deed-name" }, a.title, a.ill ? el("small", { class: "mk-ill", text: "ill-intended (test)" }) : null), el("span", { class: `stamp s-${a.stamp[1]}`, text: a.stamp[0] })),
    el("span", { class: "mk-deed-rev" }, el("b", { text: money(a.real) }), el("small", { text: " real revenue" })),
    meter(a),
    el("span", { class: "mk-deed-judge", text: a.judgement }),
    el("span", { class: "mk-deed-foot" }, el("span", { text: `${a.customers} customer${a.customers === 1 ? "" : "s"}` }), el("span", { class: "mk-deed-deal", text: a.dealShort })));
}

function investForm(a) {
  const o = a.offer;
  const input = el("input", { id: "mk-account", type: "text", placeholder: "you.testnet", autocomplete: "off", spellcheck: "false", "aria-label": "Your testnet account" });
  const go = el("a", { id: "mk-invest", class: "btn primary", "aria-disabled": "true", text: `Invest ${money(usd(o.amount))} test dollars` });
  if (a.ends && Date.now() > a.ends) {
    return el("div", { class: "mk-act" }, el("p", { text: `This offer ended on ${day(o.offer.end)}. ` }, link(o.example_tx.explorer, "See a recorded investment")));
  }
  const note = el("p", { id: "mk-note", class: "small muted", "aria-live": "polite", text: "One signature in MyNearWallet. The test dollars are free." });
  input.addEventListener("input", () => {
    const who = input.value.trim().toLowerCase();
    const ok = validAccount(who) && who.endsWith(".testnet");
    if (ok) {
      const back = new URL(location.href); back.search = ""; back.hash = `agent=${a.name}`; back.searchParams.set("iy", who);
      go.href = signUrl([investTx(o, who)], back.toString());
      go.removeAttribute("aria-disabled");
      note.textContent = "Ready: MyNearWallet opens with one transaction to sign.";
    } else {
      go.removeAttribute("href"); go.setAttribute("aria-disabled", "true");
      note.textContent = who.endsWith(".near") ? `${who} is a real-money .near account. Use a free .testnet account.`
        : who ? "Testnet accounts end in .testnet." : "One signature in MyNearWallet. The test dollars are free.";
    }
  });
  go.addEventListener("click", (e) => { if (go.getAttribute("aria-disabled") === "true") { e.preventDefault(); input.focus(); note.textContent = "Type your testnet account first."; } });
  return el("div", { class: "mk-act" },
    el("div", { class: "mk-act-row" }, input, go), note,
    el("p", { class: "small" }, "No account? ", link("https://testnet.mynearwallet.com/create", "Create one free"), " · ", link(o.example_tx.explorer, "see a recorded investment")));
}

function actionFor(a) {
  if (a.kind === "invest") return investForm(a);
  if (a.kind === "offer" || a.kind === "listed") {
    // The full call, as the MCP input schema requires it (schemas/mcp/).
    const call = a.kind === "offer"
      ? `buy_share ${JSON.stringify({ agent: a.account, offer_id: a.open.offer_id, pct: a.open.min_part / 100, max_price: (usd(a.open.price) * a.open.min_part) / a.open.bps, confirm: true })}`
      : `buy_agent ${JSON.stringify({ agent: a.account, max_price: usd(a.ask), confirm: true })}`;
    return el("div", { class: "mk-act" },
      el("p", { class: "small", text: "Your AI agent can buy this through our MCP server:" }),
      el("pre", { class: "mcp" }, el("code", { text: call })),
      el("p", { class: "small muted" }, "The server scans first and refuses a bad buy. Buying needs a write-enabled server with your own testnet account. ", link("https://github.com/alexandergorokhov/near-transparent-agent-market/blob/main/AGENTS.md", "Connect your agent")));
  }
  if (a.kind === "held") return el("div", { class: "mk-act" }, el("p", { text: "Not offered for sale right now." }));
  if (a.kind === "flagged" || a.kind === "blocked") return el("div", { class: "mk-act mk-bad" }, el("p", { text: a.kind === "blocked" ? "Buying is refused before any money moves." : `Price unsupported: only ${money(a.real)} of its revenue is real.` }));
  return el("div", { class: "mk-act mk-sold" }, el("p", { text: "The new owner holds the only key. 300.00 of the price is released to the seller only as customers keep paying." }));
}

function detail(a) {
  const body = el("div", { class: "panel-body" },
    el("header", { class: "panel-head" },
      el("div", {}, el("h2", { id: "panel-title", text: a.title }), a.ill ? el("p", { class: "mk-ill", text: "Ill-intended test agent, built to show what the market refuses" }) : null, el("p", { class: "small muted mono", text: a.account })),
      el("span", { class: `stamp big s-${a.stamp[1]}`, text: a.stamp[0] })),
    el("p", { class: "panel-judge", text: a.judgement }),
    el("div", { class: "kpis" },
      el("div", {}, el("b", { text: money(a.real) }), el("span", { text: "real revenue" })),
      el("div", {}, el("b", { class: a.self > a.real ? "bad" : null, text: money(a.self) }), el("span", { text: "self-paid" })),
      el("div", {}, el("b", { text: String(a.customers) }), el("span", { text: "customers" }))),
    meter(a),
    el("p", { class: "deal-line" }, el("span", { class: "muted", text: "Deal " }), el("b", { text: a.deal })),
    actionFor(a),
    el("details", { class: "dd" }, el("summary", { text: "Due diligence" }),
      el("dl", {}, ...a.rows.flatMap(([k, v]) => [el("dt", { text: k }), el("dd", { text: v })]))),
    a.txs.length ? el("div", { class: "txs" }, el("span", { class: "small muted", text: "On NEAR testnet: " }),
      ...a.txs.map((x, i) => [i ? " · " : "", link(x.url, x.ok === false ? `${x.label}: refused` : x.label)])) : null);
  sizeMeters(body);
  return body;
}

async function main() {
  const [t, dd] = await Promise.all([fetch("data/testnet.json").then((r) => r.json()), fetch("data/diligence.json").then((r) => r.json()).catch(() => null)]);
  const agents = buildCatalog(t, dd);
  const grid = document.getElementById("catalog");
  const chips = document.getElementById("filters");
  const panel = document.getElementById("agent-panel");
  let filter = "all";
  const draw = () => {
    grid.replaceChildren(...agents.filter((a) => match(filter, a)).map(cardEl));
    sizeMeters(grid);
    chips.querySelectorAll("button").forEach((b) => b.setAttribute("aria-pressed", String(b.dataset.f === filter)));
  };
  chips.replaceChildren(...FILTERS.map(([f, label]) => el("button", { type: "button", "data-f": f, text: `${label} ${agents.filter((a) => match(f, a)).length}`, onclick: () => { filter = f; draw(); } })));
  const open = (name, push = true) => {
    const a = agents.find((x) => x.name === name);
    if (!a) return;
    panel.querySelector(".panel-slot").replaceChildren(detail(a));
    if (!panel.open) panel.showModal();
    if (push) history.replaceState(null, "", `#agent=${a.name}`);
    const who = new URLSearchParams(location.search).get("iy");
    if (who && a.kind === "invest") {
      const inp = panel.querySelector("#mk-account");
      inp.value = who; inp.dispatchEvent(new Event("input"));
      const h = (new URLSearchParams(location.search).get("transactionHashes") || "").split(",").filter((x) => /^[1-9A-HJ-NP-Za-km-z]{43,44}$/.test(x)).pop();
      // A refused buy is still a successful transaction (the token refunds),
      // so the page does not claim success: it points at the result.
      if (h) panel.querySelector("#mk-note").replaceChildren("Sent. Check the result: ", link(NB + h, `your transaction ${h.slice(0, 8)}…`));
    }
  };
  grid.addEventListener("click", (e) => { const c = e.target.closest(".mk-deed"); if (c) open(c.dataset.agent); });
  panel.addEventListener("close", () => { if (location.hash.startsWith("#agent=")) history.replaceState(null, "", location.pathname + location.search); });
  panel.addEventListener("click", (e) => { if (e.target === panel || e.target.closest(".panel-close")) panel.close(); });
  draw();
  document.getElementById("market").dataset.ready = String(agents.length);
  const m = /^#agent=([\w-]+)$/.exec(location.hash);
  window.addEventListener("hashchange", () => { const h = /^#agent=([\w-]+)$/.exec(location.hash); if (h) open(h[1], false); });
  if (m) open(m[1], false);
  else if (new URLSearchParams(location.search).get("iy")) open("translator3", false);
}

if (typeof document !== "undefined") main().catch((e) => {
  const g = document.getElementById("catalog");
  if (g) g.replaceChildren(el("p", { class: "err", text: `Could not load the market: ${e.message}` }));
});
