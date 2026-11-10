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
  dave: { name: "dave", role: "investor", cls: "dave" },
};

// Every action: who does it, the button label, and what the contract does behind it.
const ACTIONS = {
  pay_customer: { who: "customer", label: "A customer pays 25", why: "pay(): the payer is a stranger to the owner, so the meter counts it. After a sale, half of each counted payment is released from escrow to the old seller (the earn-out)." },
  pay_verified: { who: "customer", label: "A customer pays 25 with a NEAR AI signature", why: "pay() with a NEAR AI Cloud signature on the work. The contract checks the signature on-chain, so the payment is counted and marked Verified. Here and in the offline tests the signature comes from a test key; the same check against NEAR AI Cloud's real key passed on testnet (gate G6, 2026-11-04; scenario \"Verified revenue\" below)." },
  pay_self: { who: "alice", label: "Pay your own agent 200", why: "pay(): the contract looks at who paid. The payer is the owner, so the payment is recorded but flagged and not counted. A buyer sees counted revenue only." },
  pay_sub: { who: "alice", label: "Pay from bot.alice.testnet 200", why: "NEAR account names are hierarchical: bot.alice.testnet can only be created by alice. The contract sees the parent and flags the payment." },
  plant_key: { who: "alice", label: "Hide a full-access key in the agent", why: "A NEAR account can hold many keys. alice adds one straight to the account, outside the contract. The contract only knows the keys it added itself." },
  remove_key: { who: "alice", label: "Remove the hidden key", why: "The extra key is deleted from the account." },
  toggle_upgrade: { who: "alice", label: "Deploy a build with an upgrade method", why: "An upgrade method would let the seller swap the code after the sale. The scan reads the deployed method list." },
  list: { who: "alice", label: "List the agent for 1,000", args: { price: 1000 }, why: "list_for_sale(): the price and the earn-out terms (at least 30% held, released over 30 days) are fixed in a listing version. The scan and the buy both name that version." },
  list_rev: { op: "list", who: "alice", label: "List for 1,000, paid only from revenue", args: { price: 1000, pay_from_revenue: true }, why: "list_for_sale() with the whole price held in the earn-out: alice gets nothing at the sale, only half of each real customer payment afterwards, until the 1,000 is paid or the window ends. If the agent stops earning, bob gets the rest back." },
  pay_fresh: { who: "alice", label: "Fake revenue from a fresh wallet (200)", why: "The meter cannot link a fresh wallet to alice, so the payment counts. But after the sale it releases at most half of what it pays: alice pays 200 and gets 100 back. Faking revenue after a sale always loses money." },
  commit_config: { who: "alice", label: "Commit to the agent's brain", why: "commit_config(): alice publishes the sha256 and length of the agent's off-chain part (system prompt, model, tools, which API keys to rotate). The buyer's scan shows it. The bytes stay secret until the sale." },
  deliver_config: { who: "alice", label: "Deliver the committed brain", why: "deliver_config(): alice posts the bundle encrypted to the one-time X25519 key bob sent with his buy. Until she does, no earn-out is released; if she never does, bob takes the escrow back." },
  deliver_fake: { who: "alice", label: "Deliver a different prompt", why: "alice keeps the good prompt and delivers a worse one of the same length. The contract cannot read it: it only stores the ciphertext and its hash." },
  open_config: { who: "bob", label: "Open what alice delivered", why: "bob decrypts off-chain with his one-time key and compares the sha256 with the commitment in the listing he bought." },
  dispute_config: { who: "bob", label: "Prove it on-chain", why: "dispute_config(): bob reveals his one-time secret key. The contract checks it matches the key in his buy, decrypts the stored ciphertext itself and hashes it. Not the committed bytes: the whole price (the held upfront part and the escrow) is bob's. If it had matched, the dispute would be refused." },
  claim_undelivered: { who: "bob", label: "3 days pass with no delivery", why: "claim_undelivered(): the delivery window (2 days) closed with nothing posted, so the whole price (the held upfront part and the escrow) goes back to bob." },
  accept_config: { who: "bob", label: "Accept the brain", why: "release_upfront(): bob checked the delivery and accepts it. The held upfront part (less the 1% fee) goes to alice, disputes close and earn-out releases start. If bob stays silent, anyone can call it after the 7-day dispute window." },
  cancel: { who: "alice", label: "Cancel the listing", why: "cancel_listing(): only the owner can." },
  scan: { who: "bob", label: "Scan the agent", why: "The buyer's scan reads the key list from the NEAR node (view_access_key_list) and compares it with the keys the contract added, plus the code hash, the methods and the listing terms. Anything extra blocks." },
  buy: { who: "bob", label: "Buy it", whyNo: "bob's tools (the Agent Exit MCP server and SDK) never send a buy without a clean scan of the same listing version, so nothing was paid.", why: "One NEAR transaction: bob pays, the contract checks the listing version and the key hash he scanned, then deletes alice's keys, adds bob's key and moves ownership. 70% goes to alice now, 30% is held in escrow (the default terms; in mission 5 all of it is held)." },
  buy_skip: { op: "buy", args: { skip_scan: true }, who: "bob", label: "Buy without a clean scan", why: "A contract cannot list its own account's keys, so the contract accepts the buy. The hidden key is still on the account. The next step shows what that means." },
  hidden_redeploy: { who: "alice", label: "Take the agent back with the hidden key", why: "A full-access key acts on the account itself (DeployContract, DeleteKey). Those actions never run contract code, so no contract on any chain could stop them. This is why Agent Exit puts the defence before the money moves: the buyer's scan, which the SDK and MCP server never skip." },
  withdraw: { who: "alice", label: "Withdraw the agent's money", why: "withdraw() checks that the caller is the owner. alice is not any more: NotOwner." },
  relist: { who: "alice", label: "List the agent again", why: "list_for_sale() is owner-only: NotOwner." },
  set_key: { who: "alice", label: "Add a new key for herself", why: "set_runner_key() is owner-only: NotOwner." },
  old_key: { who: "alice", label: "Use her old key", why: "The sale deleted that key from the account, so the NEAR node rejects any transaction signed with it before it reaches the contract." },
  hidden_key: { who: "alice", label: "Run the agent with the hidden key", why: "A second line of defence, not the main one: if the hidden key is ever used on the agent's own methods, the contract sees a key it never added, freezes the agent and returns the open escrow to the buyer. A careful seller would not do this; they would redeploy instead (see mission 2)." },
  wait: { who: "clock", label: "31 days pass", args: { days: 31 }, why: "finalize_earnout(): the window closed. The seller keeps what real payments released (at least the 20% floor); the rest goes back to the buyer." },
  offer_share: { who: "alice", label: "Offer 20% of future earnings for 500", why: "offer_share(): alice sells a slice of every future payment for a year, not the whole agent. Open slices can never total more than 49%, and the terms are fixed once anyone buys." },
  buy_share: { who: "dave", label: "Buy the 20% slice", why: "One ft_transfer_call with the terms dave saw (any change is refused). Half the price goes to alice now; the other half waits in escrow and reaches her only as dave gets paid, so she can't take the money and send customers elsewhere." },
  pay_customer_inv: { op: "pay_customer", who: "customer", label: "A customer pays 25", why: "pay(): 20% of the gross payment is credited to dave inside the same call, before alice's share. alice never holds it, so she can't withdraw it." },
  pay_self_inv: { op: "pay_self", who: "alice", label: "Pay your own agent 200 (fake revenue)", why: "The meter flags it and doesn't count it, dave still gets 20%, and none of dave's escrow is released to alice for it. Faking revenue costs the cheater real money." },
  claim_share: { who: "dave", label: "Claim what the agent paid", why: "claim(): pays only the caller's own balance. The contract debits first and credits back if the transfer fails." },
  end_share: { who: "clock", label: "A year passes; close the slice", why: "close_share(): anyone can call it after the end date. The escrow not yet released goes back to dave, and future payments go to the owner again." },
};

// Where each action lives in the real system. `code`: the Rust function the
// wasm ran (file, line, signature start; the smoke test checks the line).
// `near`: what it is on NEAR. `tx`: the label of the same call in the
// recorded testnet run (data/testnet.json), linked to NearBlocks.
const REPO = "https://github.com/alexandergorokhov/near-transparent-agent-market/blob/main/";
const CORE = "contracts/agent-core/src/lib.rs";
const CODE = {
  pay_customer: { code: [CORE, 1376, "pub fn pay("], near: "xusd ft_transfer_call -> agent ft_on_transfer (kind pay)", tx: "p1 pays 25.00 for a job" },
  pay_verified: { code: [CORE, 1376, "pub fn pay("], near: "ft_transfer_call with a NEAR AI Cloud signature, checked by the contract (ecrecover)", tx: null, note: "On testnet gate G6 passed (2026-11-04) with a real NEAR AI Cloud signature: see the scenario \"Verified revenue\" below." },
  pay_self: { code: [CORE, 1353, "pub fn flags_for("], near: "ft_transfer_call from the owner's own account", tx: null },
  pay_sub: { code: [CORE, 1353, "pub fn flags_for("], near: "ft_transfer_call from bot.alice.testnet (a sub-account only alice can create)", tx: null },
  plant_key: { code: null, near: "an AddKey action on the agent's account, signed by a full-access key: plain NEAR protocol, no contract involved", tx: "alice lists cheat at 500.00", note: "The cheat agent on testnet carries such a key." },
  remove_key: { code: null, near: "a DeleteKey action on the agent's account", tx: null },
  toggle_upgrade: { code: ["tools/scan/src/lib.rs", 153, "pub const UPGRADE_WORDS"], near: "a different wasm deployed on the agent's account", tx: null },
  list: { code: [CORE, 935, "pub fn list("], near: "agent.list_for_sale(price, code_hash), signed by the owner", tx: "alice lists translator at 1000.00" },
  cancel: { code: [CORE, 979, "pub fn cancel_listing("], near: "agent.cancel_listing()", tx: null },
  scan: { code: ["tools/scan/src/lib.rs", 477, "pub fn scan("], near: "read-only RPC: view_access_key_list, view_account (code hash), get_status, get_listing; run by the buyer's SDK and the MCP server's scan_agent tool", tx: null },
  buy: { code: [CORE, 997, "pub fn begin_buy("], near: "xusd.ft_transfer_call(agent, price, {listing_version, known_keys_hash, buyer key}) -> one key batch on the agent's account (DeleteKey seller, AddKey buyer) -> on_handover callback", tx: "bob buys translator (one ft_transfer_call)" },
  buy_skip: { code: [CORE, 997, "pub fn begin_buy("], near: "the same ft_transfer_call, sent without the scan", tx: null },
  withdraw: { code: [CORE, 1288, "pub fn withdraw("], near: "agent.withdraw(), signed by alice", tx: "alice: withdraw 1.00" },
  list_rev: { code: [CORE, 935, "pub fn list("], near: "agent.list_for_sale(price, code_hash, terms {earnout_bps: 10000, floor_bps: 0}), signed by the owner", tx: null },
  pay_fresh: { code: [CORE, 1376, "pub fn pay("], near: "ft_transfer_call from a wallet with no on-chain link to the seller", tx: null },
  commit_config: { code: [CORE, 1499, "pub fn commit_config("], near: "agent.commit_config(config_hash, bytes), signed by the owner", tx: null },
  deliver_config: { code: [CORE, 1618, "pub fn deliver_config("], near: "agent.deliver_config(sale_id, ephemeral_pk, ciphertext), signed by the seller", tx: null },
  deliver_fake: { code: [CORE, 1618, "pub fn deliver_config("], near: "agent.deliver_config(...) with other bytes", tx: null },
  open_config: { code: ["contracts/agent-core/src/sealed.rs", 379, "pub fn open("], near: "off-chain, in the buyer's tools", tx: null },
  dispute_config: { code: [CORE, 1669, "pub fn dispute_config("], near: "agent.dispute_config(sale_id, delivery_sk), signed by the buyer; X25519 + SHA-256 run inside the contract", tx: null },
  claim_undelivered: { code: [CORE, 1655, "pub fn claim_undelivered("], near: "agent.claim_undelivered(sale_id), anyone, after the deadline", tx: null },
  accept_config: { code: [CORE, 1712, "pub fn release_upfront("], near: "agent.release_upfront(sale_id), signed by the buyer (or anyone after the dispute window)", tx: null },
  relist: { code: [CORE, 935, "pub fn list("], near: "agent.list_for_sale(), signed by alice", tx: "alice: list_for_sale 1.00" },
  set_key: { code: [CORE, 771, "pub fn set_runner_key("], near: "agent.set_runner_key(), signed by alice", tx: "alice: set_runner_key (her own key)" },
  old_key: { code: null, near: "any transaction signed with the deleted key: the NEAR node refuses it (AccessKeyNotFound)", tx: "alice's old runner key: heartbeat" },
  hidden_key: { code: [CORE, 1897, "pub fn runner_guard("], near: "agent.heartbeat() signed by a key the contract never added", tx: null, note: "Proved on a real node in the sandbox test A23 (tests/sandbox)." },
  hidden_redeploy: { code: null, near: "a transaction with DeployContract and DeleteKey actions signed by the hidden full-access key: plain NEAR protocol, no contract code runs", tx: null, note: "Listed in docs/NOT-CAUGHT.md as N4: the scan is the defence." },
  wait: { code: [CORE, 1767, "pub fn finalize_earnout("], near: "agent.finalize_earnout() after the 30-day window", tx: null },
  offer_share: { code: [CORE, 2045, "pub fn offer_share("], near: "agent.offer_share(bps, price, end), signed by the owner with 1 yoctoNEAR", tx: null, note: "Rules SH1-SH11 and threats T1-T17: docs/PLAN-shares.md; tests in contracts/agent-core/tests/shares.rs." },
  buy_share: { code: [CORE, 2117, "pub fn buy_share("], near: "xusd.ft_transfer_call(agent, cost, {kind: buy_share, offer_id, bps, price, end, part})", tx: null },
  pay_customer_inv: { code: [CORE, 2006, "fn pay_investors("], near: "xusd ft_transfer_call -> agent ft_on_transfer (kind pay); events split.to_investors and share_paid", tx: null },
  pay_self_inv: { code: [CORE, 2006, "fn pay_investors("], near: "ft_transfer_call from the owner's own account", tx: null },
  claim_share: { code: [CORE, 1273, "pub fn claim("], near: "agent.claim(), signed by dave", tx: null },
  end_share: { code: [CORE, 2189, "pub fn close_share("], near: "agent.close_share(offer_id), anyone, after the end date", tx: null },
};

let testnetTxs = [];

function realBlock(key, request, last) {
  const c = CODE[key];
  const rows = [];
  if (c.code) {
    const [file, line] = c.code;
    rows.push(el("li", {}, el("span", { class: "rk", text: "Code that ran" }),
      el("a", { href: `${REPO}${file}#L${line}`, target: "_blank", rel: "noopener noreferrer", text: `${file}:${line}` })));
  }
  rows.push(el("li", {}, el("span", { class: "rk", text: "On NEAR" }), el("span", { text: c.near })));
  const t = c.tx && testnetTxs.find((x) => x.label === c.tx);
  if (t) {
    rows.push(el("li", {}, el("span", { class: "rk", text: "Same call on testnet" }),
      t.explorer ? el("a", { href: t.explorer, target: "_blank", rel: "noopener noreferrer", text: `${t.label} (NearBlocks)` })
        : el("span", { text: `${t.label}: ${t.error || "refused"} (rejected before a transaction exists)` })));
  }
  if (c.note) rows.push(el("li", {}, el("span", { class: "rk", text: "Note" }), el("span", { text: c.note })));
  return el("details", { class: "real" }, el("summary", { text: "How this works for real" }),
    el("ul", { class: "real-rows" }, rows),
    el("p", { class: "rk", text: "Raw call into the wasm, and its answer" }),
    el("pre", { text: `${JSON.stringify(request)}\n-> ${JSON.stringify(last)}` }));
}

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
    steps: ["plant_key", "list", "scan", "buy", "buy_skip", "hidden_redeploy"],
    lesson: "The scan is the defence: it read the account's real key list and blocked the sale before any money moved. Skip it, and a hidden full-access key gives the agent back to the seller; no contract can prevent that. The Agent Exit SDK and MCP server never buy without a clean scan.",
  },
  {
    id: "honest",
    title: "3. An honest sale",
    intro: "A clean sale. bob buys the agent, then alice tries every way back in.",
    steps: ["list", "scan", "buy", "withdraw", "set_key", "old_key", "pay_customer", "wait"],
    lesson: "One transaction moved the agent. Every way back in is refused, and alice still earns her escrow from real customers.",
  },
  {
    id: "invest",
    title: "4. Invest in the agent",
    intro: "Instead of selling the whole agent, alice sells 20% of its future earnings. You are dave, the investor. The same meter that a buyer checks pays you.",
    steps: ["offer_share", "buy_share", "pay_customer_inv", "pay_self_inv", "claim_share", "end_share"],
    lesson: "dave was paid inside every payment, including alice's fake one, and alice could never touch his part. If the agent stops earning, the escrow comes back to him at the end.",
  },
  {
    id: "revenue",
    title: "5. Pay from revenue",
    intro: "alice sells, but bob pays her only out of the agent's future revenue. Can alice fake that revenue to get paid?",
    steps: ["list_rev", "scan", "buy", "pay_fresh", "pay_customer", "wait"],
    lesson: "Nothing went to alice at the sale. A fake payment from a wallet nobody can trace to her still cost her half of it. Only real customers paid her; the rest of the price went back to bob.",
  },
  {
    id: "brain",
    title: "6. Buy the brain too",
    intro: "The agent's real value is off-chain: its prompt, model and tool setup. alice sells it sealed. Can she deliver a worse one and keep the money?",
    steps: ["commit_config", "list", "scan", "buy", "pay_customer", "deliver_fake", "open_config", "dispute_config"],
    lesson: "The contract never saw the prompt, yet it proved the swap: bob revealed a one-time key, the contract decrypted and hashed the delivery itself, and the whole price went back to him. alice was paid nothing at the sale: the upfront part waits until bob accepts the delivery (or 7 days pass with no dispute). She can still keep a copy of the prompt, so bob rotates the API keys the bundle names.",
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

function feedItem(key, last, request) {
  const a = ACTIONS[key];
  return el("li", { class: `feed-item ${last.ok ? "ok" : "no"}`, "data-action": key },
    el("div", { class: "feed-head" }, who(a.who), el("b", { text: a.label })),
    el("p", { class: "result", text: last.text }),
    el("p", { class: "behind" }, el("span", { class: "behind-label", text: "Behind the scenes: " }), (!last.ok && a.whyNo) || a.why),
    realBlock(key, request, last));
}

function run(key) {
  const a = ACTIONS[key];
  const request = { op: a.op || key, ...(a.args || {}) };
  const last = call(request);
  done.add(key);
  const feed = document.getElementById("pg-feed");
  feed.prepend(feedItem(key, last, request));
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
  const status = v.taken_over ? "Taken back" : v.frozen ? "Frozen" : v.status;
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
      v.sale ? [el("dt", { text: "Upfront part (before the 1% fee)" }), el("dd", { text: v.sale.upfront })] : null,
      v.sale && v.sale.upfront_held !== "0.00" ? [el("dt", { text: "Upfront held until bob accepts" }), el("dd", { text: v.sale.upfront_held })] : null,
      el("dt", { text: "Escrow left" }), el("dd", { text: v.escrow_left }),
      el("dt", { text: "alice can claim" }), el("dd", { text: v.claimable.alice }),
      el("dt", { text: "bob can claim" }), el("dd", { text: v.claimable.bob })));
  }
  if (v.shares.length || v.claimable.dave !== "0.00") {
    const rows = v.shares.flatMap((o) => [
      el("dt", { text: `Slice ${o.id}` }), el("dd", { text: `${o.pct}% of every payment, ${o.sold_pct}% sold, for ${o.price}` }),
      ...o.holders.flatMap((h) => [
        el("dt", { text: `${h.account.split(".")[0]} paid / earned` }), el("dd", { text: `${h.paid_in} / ${h.earned}` }),
        el("dt", { text: "Escrow still held" }), el("dd", { text: h.escrow })]),
    ]);
    kids.push(el("h4", { text: "Investors" }), el("dl", { class: "facts-grid" }, rows,
      el("dt", { text: "dave can claim" }), el("dd", { class: "dave-claim", text: v.claimable.dave })));
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
  document.getElementById("free-actions").replaceChildren(...Object.keys(ACTIONS).filter((k) => !k.endsWith("_inv")).map((k) => {
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
  try {
    const t = await (await fetch("data/testnet.json")).json();
    testnetTxs = Object.values(t.steps).flatMap((x) => x.txs);
  } catch { /* the testnet links are optional */ }
  call({ op: "view" });
  renderState();
  renderMission();
  renderFree();
  document.getElementById("play").dataset.ready = "1";
}

main().catch((e) => {
  document.getElementById("pg-state").replaceChildren(el("p", { class: "err", text: `Could not start the playground: ${e.message}` }));
});
