// "Try it yourself": the page loads playground.wasm, which is the contract's
// own state machine (contracts/agent-core) and the buyer's scan (tools/scan)
// compiled from the same Rust (tools/playground, `make playground-wasm`).
// This file only draws buttons and the state; every answer comes from wasm.

function el(tag, attrs = {}, ...kids) {
  const n = document.createElement(tag);
  for (const [k, v] of Object.entries(attrs)) {
    if (v == null || v === false) continue;
    if (k === "class") n.className = v;
    else if (k === "text") n.textContent = v;
    else n.setAttribute(k, v === true ? "" : v);
  }
  for (const k of kids.flat()) if (k != null) n.append(k.nodeType ? k : String(k));
  return n;
}

const WHO = {
  alice: { name: "alice", role: "seller", cls: "alice" },
  bob: { name: "bob", role: "buyer", cls: "bob" },
  customer: { name: "a customer", role: "customer", cls: "cust" },
  clock: { name: "time", role: "", cls: "clock" },
};

// Every action: who does it, the button label, and what the contract does behind it.
const ACTIONS = {
  pay_customer: { who: "customer", label: "A customer pays 25", why: "pay(): the payer is a stranger to the owner, so the meter counts it. After a sale, half of each counted payment is released from escrow to the old seller (the earn-out)." },
  pay_verified: { who: "customer", label: "A customer pays 25 with a NEAR AI signature", why: "pay() with a NEAR AI Cloud signature on the work. The contract checks the signature on-chain, so the payment is counted and marked Verified." },
  pay_self: { who: "alice", label: "Pay your own agent 200", why: "pay(): the contract looks at who paid. The payer is the owner, so the payment is recorded but flagged and not counted. A buyer sees counted revenue only." },
  pay_sub: { who: "alice", label: "Pay from bot.alice.testnet 200", why: "NEAR account names are hierarchical: bot.alice.testnet can only be created by alice. The contract sees the parent and flags the payment." },
  plant_key: { who: "alice", label: "Hide a full-access key in the agent", why: "A NEAR account can hold many keys. alice adds one straight to the account, outside the contract. The contract only knows the keys it added itself." },
  remove_key: { who: "alice", label: "Remove the hidden key", why: "The extra key is deleted from the account." },
  toggle_upgrade: { who: "alice", label: "Deploy a build with an upgrade method", why: "An upgrade method would let the seller swap the code after the sale. The scan reads the deployed method list." },
  list: { who: "alice", label: "List the agent for 1,000", args: { price: 1000 }, why: "list_for_sale(): the price and the earn-out terms (at least 30% held, released over 30 days) are fixed in a listing version. The scan and the buy both name that version." },
  cancel: { who: "alice", label: "Cancel the listing", why: "cancel_listing(): only the owner can." },
  scan: { who: "bob", label: "Scan the agent", why: "The buyer's scan reads the key list from the NEAR node (view_access_key_list) and compares it with the keys the contract added, plus the code hash, the methods and the listing terms. Anything extra blocks." },
  buy: { who: "bob", label: "Buy it", whyNo: "bob's tools (the Agent Exit MCP server and SDK) never send a buy without a clean scan of the same listing version, so nothing was paid.", why: "One NEAR transaction: bob pays, the contract checks the listing version and the key hash he scanned, then deletes alice's keys, adds bob's key and moves ownership. 70% goes to alice now, 30% is held in escrow." },
  buy_skip: { op: "buy", args: { skip_scan: true }, who: "bob", label: "Buy without a clean scan", why: "The contract itself cannot list the account's keys, so a buyer who skips the scan can buy an agent with a hidden key. The next step shows what that key is worth to the seller." },
  withdraw: { who: "alice", label: "Withdraw the agent's money", why: "withdraw() checks that the caller is the owner. alice is not any more: NotOwner." },
  relist: { who: "alice", label: "List the agent again", why: "list_for_sale() is owner-only: NotOwner." },
  set_key: { who: "alice", label: "Add a new key for herself", why: "set_runner_key() is owner-only: NotOwner." },
  old_key: { who: "alice", label: "Use her old key", why: "The sale deleted that key from the account, so the NEAR node rejects any transaction signed with it before it reaches the contract." },
  hidden_key: { who: "alice", label: "Use the hidden key", why: "The agent's runner methods check which key signed. A key the contract never added proves a backdoor: the agent freezes and the open escrow goes back to the buyer. The seller loses the 300." },
  wait: { who: "clock", label: "31 days pass", args: { days: 31 }, why: "finalize_earnout(): the window closed. The seller keeps what real payments released (at least the 20% floor); the rest goes back to the buyer." },
};

const MISSIONS = [
  {
    id: "fake",
    title: "1. Fake the revenue",
    intro: "You are alice. You want to sell your translator agent for more, so you try to inflate its revenue.",
    steps: ["pay_self", "pay_sub", "pay_customer", "pay_verified"],
    lesson: "Only payments from real customers count. Your own 400 shows as flagged, and the buyer sees it.",
  },
  {
    id: "backdoor",
    title: "2. Hide a backdoor",
    intro: "You are alice again. This time you plan to sell the agent and keep a secret way back in.",
    steps: ["plant_key", "list", "scan", "buy", "buy_skip", "hidden_key"],
    lesson: "The scan blocks the sale. If a buyer skips it, using the hidden key freezes the agent and sends the 300 escrow back to the buyer, so the backdoor costs the seller money.",
  },
  {
    id: "honest",
    title: "3. An honest sale",
    intro: "A clean sale. bob buys the agent, then alice tries every way back in.",
    steps: ["list", "scan", "buy", "withdraw", "set_key", "old_key", "pay_customer", "wait"],
    lesson: "One transaction moved the agent. Every way back in is refused, and alice still earns her escrow from real customers.",
  },
];

let wasm;
let view;
let mission = MISSIONS[0];
let done = new Set();

function call(action) {
  const enc = new TextEncoder();
  const bytes = enc.encode(JSON.stringify(action));
  const p = wasm.pg_alloc(bytes.length);
  new Uint8Array(wasm.memory.buffer, p, bytes.length).set(bytes);
  const n = wasm.pg_call();
  const out = JSON.parse(new TextDecoder().decode(new Uint8Array(wasm.memory.buffer, wasm.pg_out(), n)));
  if (!out.ok) throw new Error(out.error);
  view = out.view;
  return out.last;
}

function who(k) {
  const w = WHO[k];
  return el("span", { class: `who ${w.cls}` }, w.name, w.role ? el("small", { text: ` ${w.role}` }) : null);
}

function feedItem(key, last) {
  const a = ACTIONS[key];
  return el("li", { class: `feed-item ${last.ok ? "ok" : "no"}`, "data-action": key },
    el("div", { class: "feed-head" }, who(a.who), el("b", { text: a.label })),
    el("p", { class: "result", text: last.text }),
    el("p", { class: "behind" }, el("span", { class: "behind-label", text: "Behind the scenes: " }), (!last.ok && a.whyNo) || a.why));
}

function run(key) {
  const a = ACTIONS[key];
  const last = call({ op: a.op || key, ...(a.args || {}) });
  done.add(key);
  const feed = document.getElementById("pg-feed");
  feed.prepend(feedItem(key, last));
  renderState();
  renderMission();
}

function reset() {
  call({ op: "reset" });
  done = new Set();
  document.getElementById("pg-feed").replaceChildren();
  renderState();
  renderMission();
}

function bar(label, value, max, cls) {
  const v = Number(String(value).replace(/,/g, ""));
  const pct = max > 0 ? Math.min(100, (v / max) * 100) : 0;
  const fill = el("span", { class: `fill ${cls}` });
  fill.style.width = `${pct}%`; // CSSOM, allowed by the CSP (a style attribute is not)
  return el("div", { class: "bar-row" },
    el("span", { class: "bar-label", text: label }),
    el("span", { class: "bar" }, fill),
    el("span", { class: "bar-v", text: value }));
}

function renderState() {
  const v = view;
  const e = v.earnings;
  const gross = Number(e.gross.replace(/,/g, ""));
  const status = v.frozen ? "Frozen" : v.status;
  const keys = el("ul", { class: "keys" }, v.keys.map((k) => el("li", { class: k.known_to_contract ? "" : "danger" },
    el("span", { class: "key-name", text: k.label }),
    el("span", { class: `chip ${k.full_access ? "red" : ""}`, text: k.full_access ? "full access" : "runs the agent only" }),
    el("span", { class: `chip ${k.known_to_contract ? "green" : "red"}`, text: k.known_to_contract ? "added by the contract" : "unknown to the contract" }))));
  const kids = [
    el("div", { class: "agent-card" },
      el("div", {}, el("div", { class: "agent-name", text: "translator.testnet" }), el("div", { class: "muted small", text: "an AI translation agent" })),
      el("span", { class: `status ${status.toLowerCase()}`, text: status })),
    el("dl", { class: "facts-grid" },
      el("dt", { text: "Owner" }), el("dd", { class: "owner", text: v.owner }),
      el("dt", { text: "Listing" }), el("dd", { text: v.listing ? `${v.listing.price} xUSD (version ${v.listing.version})` : "not listed" })),
    el("h4", { text: "Earnings the buyer sees" }),
    bar("counted", e.counted, gross, "green"),
    bar("verified", e.verified, gross, "blue"),
    bar("flagged", e.flagged, gross, "red"),
    el("h4", { text: "Keys on the agent's NEAR account" }),
    keys,
  ];
  if (v.scan) {
    const s = v.scan;
    kids.push(el("h4", { text: "Last scan" }), el("div", { class: `scan ${s.verdict}` },
      el("b", { text: s.verdict === "clean" ? "Clean: safe to buy" : `Blocked (${s.verdict})` }),
      s.findings.length ? el("ul", {}, s.findings.map((f) => el("li", {}, el("code", { text: f.code }), ` ${f.detail}`))) : null));
  }
  if (v.sale || v.claimable.bob !== "0.00" || v.claimable.alice !== "0.00") {
    kids.push(el("h4", { text: "The sale" }), el("dl", { class: "facts-grid" },
      v.sale ? [el("dt", { text: "Paid to alice now" }), el("dd", { text: v.sale.paid_now })] : null,
      el("dt", { text: "Escrow left" }), el("dd", { text: v.escrow_left }),
      el("dt", { text: "alice can claim" }), el("dd", { text: v.claimable.alice }),
      el("dt", { text: "bob can claim" }), el("dd", { text: v.claimable.bob })));
  }
  document.getElementById("pg-state").replaceChildren(...kids);
}

function renderMission() {
  document.getElementById("missions").replaceChildren(...MISSIONS.map((m) => {
    const b = el("button", { type: "button", role: "tab", class: "mission-tab", "aria-selected": m === mission ? "true" : "false", "data-mission": m.id, text: m.title });
    b.addEventListener("click", () => { mission = m; reset(); });
    return b;
  }));
  document.getElementById("mission-intro").textContent = mission.intro;
  const next = mission.steps.find((k) => !done.has(k));
  document.getElementById("mission-steps").replaceChildren(...mission.steps.map((k, i) => {
    const a = ACTIONS[k];
    const state = done.has(k) ? "done" : k === next ? "next" : "later";
    const b = el("button", { type: "button", class: `act ${state}`, "data-action": k, disabled: state === "later" },
      el("span", { class: "num", text: done.has(k) ? "✓" : String(i + 1) }), who(a.who), el("span", { class: "act-label", text: a.label }));
    b.addEventListener("click", () => run(k));
    return el("li", {}, b);
  }));
  const lesson = document.getElementById("mission-lesson");
  lesson.hidden = !mission.steps.every((k) => done.has(k));
  lesson.textContent = mission.lesson;
}

function renderFree() {
  document.getElementById("free-actions").replaceChildren(...Object.keys(ACTIONS).map((k) => {
    const b = el("button", { type: "button", class: "free-act", "data-action": k }, who(ACTIONS[k].who), ` ${ACTIONS[k].label}`);
    b.addEventListener("click", () => run(k));
    return b;
  }), (() => { const r = el("button", { type: "button", class: "free-act", text: "Start over" }); r.addEventListener("click", reset); return r; })());
}

async function main() {
  const res = await fetch("playground.wasm");
  if (!res.ok) throw new Error(`playground.wasm: HTTP ${res.status}`);
  const { instance } = await WebAssembly.instantiate(await res.arrayBuffer(), {});
  wasm = instance.exports;
  call({ op: "view" });
  renderState();
  renderMission();
  renderFree();
  document.getElementById("play").dataset.ready = "1";
}

main().catch((e) => {
  document.getElementById("pg-state").replaceChildren(el("p", { class: "err", text: `Could not start the playground: ${e.message}` }));
});
