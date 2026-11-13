// "Run a scenario on NEAR testnet": each scenario is a recorded run of the
// deployed contract (data/testnet.json, from out/scenes). Running it opens
// its stages one by one and re-checks each against the chain through the
// page's read-only relay: the transaction's signer, method, outcome and the
// contract's event logs, or the agent's live key list for a scan. The page
// shows what the chain answers, next to what was recorded.

function el(tag, attrs = {}, ...kids) {
  const n = document.createElement(tag);
  for (const [k, v] of Object.entries(attrs)) {
    if (v == null || v === false) continue;
    if (k === "class") n.className = v;
    else if (k === "text") n.textContent = v;
    else n.setAttribute(k, v === true ? "" : v);
  }
  for (const k of kids.flat(3)) if (k != null) n.append(k.nodeType ? k : String(k));
  return n;
}

const SCENARIOS = [
  { id: "sale", title: "Sell an agent: the seller's key is deleted", steps: ["honest-sale", "break-in"],
    intro: "alice lists Polyglot, bob buys it in one transaction, then alice tries every way back in." },
  { id: "cheat", title: "A cheat is caught before the sale", steps: ["cheat"],
    intro: "alice lists an agent that carries a hidden full-access key. bob's scan reads the account's keys and refuses to buy." },
  { id: "business", title: "The agent runs inside its limits", steps: ["business"],
    intro: "bob, the new owner, sets a daily spending cap. Customers pay the agent; the agent pays a supplier; an over-cap payment is refused." },
  { id: "agent", title: "An AI agent buys an agent", steps: ["agent-buys-agent"],
    intro: "summarizer, itself an agent account, scans Polyglot and buys it from bob through the same contract." },
  { id: "invest", title: "Invest in an agent: 20% of its earnings", steps: ["invest"],
    intro: "alice offers 20% of Lingo Live's earnings for a year; dave scans it and buys. Five customers pay for real NEAR AI translations, each signature checked by the contract; the owner's own sub-account pays too and is flagged. dave gets exactly 20% of every payment and claims it." },
  { id: "born", title: "Born clean: an agent no one ever held a key to", steps: ["born-clean"],
    intro: "The factory creates the agent account and deploys the published code in one transaction, so no seller key ever existed. The buyer's scan reads the factory's record." },
  { id: "sealed", title: "The seller swaps the prompt: the whole price comes back", steps: ["sealed-prompt"],
    intro: "alice commits to the agent's prompt bundle by hash and Merkle root. bob sees two random chunks proven against it, then buys with a one-time key. alice delivers a different bundle; bob reveals the key, the contract decrypts and hashes it on-chain, and bob gets the whole price back. alice is paid nothing." },
  { id: "day", title: "A business day: the owner only sets the constraints", steps: ["business-day"],
    intro: "alice signs one transaction: a daily cap of 8.00, one allowed supplier, 80% of revenue to the treasury. Then no human signs: five customers pay, the agent's own runner key buys a supplier job after each until the cap refuses the fifth, a payment to anyone else is refused, and dave, a 20% investor, is paid inside every payment." },
  { id: "nearai", title: "Verified revenue: a NEAR AI signature checked on-chain", steps: ["nearai-check"],
    intro: "A payment carries NEAR AI Cloud's TEE signature over the request and the answer. The contract recovers the signer and marks the payment verified." },
  { id: "aibuyer", title: "A real AI model buys an agent through MCP", steps: ["ai-buyer"],
    intro: "Claude, given only the Agent Exit skill and MCP tools, refused the agent with a hidden key and bought the clean one. The server signs; the model never sees a key." },
];

async function rpc(method, params) {
  const r = await fetch("api/rpc", { method: "POST", headers: { "content-type": "application/json" },
    body: JSON.stringify({ jsonrpc: "2.0", id: "verify", method, params }) });
  const j = await r.json();
  if (j.error) {
    const e = new Error(j.error.message || j.error.name || JSON.stringify(j.error).slice(0, 160));
    const c = j.error.cause && j.error.cause.name;
    if (c === "UNKNOWN_TRANSACTION" || /UNKNOWN_TRANSACTION/.test(JSON.stringify(j.error))) e.notFound = true;
    throw e;
  }
  return j.result;
}

function chainEvents(res) {
  const out = [];
  for (const o of res.receipts_outcome || []) {
    for (const l of o.outcome.logs || []) {
      if (!l.startsWith("EVENT_JSON:")) continue;
      try { out.push(JSON.parse(l.slice(11)).event); } catch { /* not ours */ }
    }
  }
  return out;
}

function failureText(status) {
  if (!status || !status.Failure) return null;
  const s = JSON.stringify(status.Failure);
  const m = /"(?:FunctionCallError|ExecutionError)":"?([^"}]+)/.exec(s) || /PanicMsg[^:]*:\\?"?([^"\\]+)/.exec(s);
  return m ? m[1].slice(0, 140) : "rejected by the contract"; // never a cut-off JSON fragment
}

/** The recorder's label for an action batch (same rule as mcp/src/backend.ts txLabel). */
function txLabel(actions) {
  const has = (k) => actions.some((a) => a === k || (a && typeof a === "object" && k in a));
  const fc = actions.map((a) => a && a.FunctionCall && a.FunctionCall.method_name).find(Boolean);
  if (fc) return has("DeployContract") ? `deploy+${fc}` : fc;
  if (has("CreateAccount")) return "create_account";
  if (has("DeployContract")) return "deploy";
  if (has("DeleteKey")) return "delete_key";
  if (has("AddKey")) return "add_key";
  return "(no call)";
}

/** Re-checks one recorded transaction against the chain; returns rows and a verdict. */
async function verifyTx(t) {
  const res = await rpc("tx", { tx_hash: t.tx, sender_account_id: t.by, wait_until: "NONE" });
  const tx = res.transaction || {};
  const call = txLabel(tx.actions || []);
  const ok = res.status && "SuccessValue" in res.status;
  // A call can succeed at the top and still be refused inside (a token refund): the contract's own events say which.
  const events = chainEvents(res);
  const refusedInside = (res.receipts_outcome || []).some((o) => o.outcome.status && o.outcome.status.Failure);
  const matches = tx.signer_id === t.by && tx.receiver_id === t.receiver && call === t.method
    && (t.ok ? ok : (!ok || refusedInside)) && t.events.every((e) => events.includes(e));
  return {
    matches,
    rows: [
      ["Signed by", tx.signer_id],
      ["Called", `${tx.receiver_id}.${call}`],
      ["Outcome on chain", ok ? (refusedInside ? "success at the top, refused inside (refunded)" : "success") : `failed: ${failureText(res.status)}`],
      ["Contract events", events.length ? events.join(", ") : "none"],
      ["In block", (res.transaction_outcome || {}).block_hash],
    ],
  };
}

/** Re-checks a recorded scan: the flagged key is on the agent's account right now, with full access. */
async function verifyScan(scan) {
  const res = await rpc("query", { request_type: "view_access_key_list", account_id: scan.agent, finality: "final" });
  const full = res.keys.filter((k) => k.access_key.permission === "FullAccess").map((k) => k.public_key);
  const rows = [["Keys on the account now", `${res.keys.length} (${full.length} full access)`]];
  if (scan.verdict === "clean") {
    // A clean scan said: no full-access key. The account must still have none.
    rows.push(["Full-access keys", full.length ? full.join(", ") : "none, as the scan found"]);
    return { matches: full.length === 0, rows };
  }
  const flagged = scan.findings.map((f) => (/(ed25519:[1-9A-HJ-NP-Za-km-z]+)/.exec(f.detail) || [])[1]).filter(Boolean);
  rows.push(["The flagged key", flagged.length ? `${flagged[0]} ${full.includes(flagged[0]) ? "is there, with full access" : "is NOT there"}` : "none named"]);
  return { matches: flagged.length > 0 && flagged.every((k) => full.includes(k)), rows };
}

function stagesFor(data, stepIds) {
  const stages = [];
  for (const id of stepIds) {
    const s = data.steps[id];
    if (!s) continue;
    // The scan happens just before the buy (an ft_transfer_call); with no buy, it ends the step.
    const buyAt = s.txs.findIndex((t) => t.method === "ft_transfer_call");
    s.txs.forEach((t, i) => {
      if (s.scan && i === buyAt) stages.push({ kind: "scan", scan: s.scan });
      stages.push({ kind: t.tx ? "tx" : "nokey", t });
    });
    if (s.scan && buyAt < 0) stages.push({ kind: "scan", scan: s.scan });
  }
  return stages;
}

/** A curl the reader can run to ask a public archival node, not our relay. */
function selfCheck(t) {
  const body = JSON.stringify({ jsonrpc: "2.0", id: 1, method: "tx", params: { tx_hash: t.tx, sender_account_id: t.by, wait_until: "NONE" } });
  return `curl -s https://archival-rpc.testnet.near.org -H 'content-type: application/json' \
  -d '${body}'`;
}

/** "translator3.factory.agentexit6.testnet" -> "translator3": the first part is the name people read. */
const shortAcc = (text) => String(text).replace(/\b([a-z0-9_-]+)(?:\.[a-z0-9_-]+)*\.testnet\b/g, "$1");

/** Plain words for the recorded labels; the method name stays in the stage's monospace hint. */
const PLAIN = [
  [/\(one ft_transfer_call\)/, "(one transaction)"],
  [/set_runner_key \(her own key\)/, "puts her own runner key back"],
  [/list_for_sale ([\d.]+)/, "relists it for $1"],
  [/: set_limits/, ": changes its spending limits"],
  [/nft_transfer the deed to herself/, "takes the deed back"],
  [/pay_base \(the Base wallet\)/, "spends from its Base wallet"],
  [/runner key: heartbeat/, "runner key: signs a call"],
  [/setup key: heartbeat/, "setup key: signs a call"],
  [/alice sets her runner key/, "alice sets her runner key"],
  [/registry admin: add_signer \(NEAR AI Cloud signing address\)/, "registry admin trusts NEAR AI Cloud's signing address"],
];
// People read brand names and capitalised persons; account ids stay in the hint and the links.
const BRAND = [[/\btranslator3\b/g, "Lingo Live"], [/\btranslator\b/g, "Polyglot"], [/\balice\b/g, "Alice"], [/\bbob\b/g, "Bob"], [/\bdave\b/g, "Dave"]];
const plain = (text) => BRAND.reduce((t, [re, w]) => t.replace(re, w), PLAIN.reduce((t, [re, w]) => t.replace(re, w), shortAcc(text)));

function stageView(st, i) {
  const head = plain(st.kind === "scan"
    ? `buyer's scan of ${st.scan.agent}: ${st.scan.verdict}`
    : st.t.label);
  const recorded = st.kind === "scan"
    ? `${st.scan.findings.map((f) => `${f.code}: ${f.detail}`).join("; ") || "no findings"}. A read of the account, not a transaction.${st.scan.verdict === "clean" ? "" : " No buy was sent."}`
    : st.kind === "nokey"
      ? `Refused, as expected: her deleted key could not even sign (${st.t.error || "the node rejected the key"}).`
      : `${st.t.ok ? "accepted" : `refused: ${st.t.error}`}${st.t.events.length ? `; events ${st.t.events.join(", ")}` : ""}`;
  const pill = el("span", { class: "vpill wait", text: "recorded" });
  const body = el("div", { class: "stage-body" },
    el("p", { class: "stage-rec" }, el("b", { text: "Recorded: " }), recorded),
    st.kind === "tx" ? el("p", { class: "stage-call small" },
      el("a", { href: st.t.explorer, target: "_blank", rel: "noopener noreferrer", text: "Transaction on NearBlocks ↗" }), " ",
      el("code", { class: "hint", title: `${st.t.by} → ${st.t.receiver}.${st.t.method}()`, text: `${st.t.method}()` })) : null,
    // Review 5: the check above goes through this page's relay; this one does not.
    st.kind === "tx" ? el("details", { class: "selfcheck" }, el("summary", { text: "Check it without this page" }),
      el("p", { class: "muted small", text: "Ask a public NEAR archival node yourself:" }),
      el("pre", {}, el("code", { text: selfCheck(st.t) }))) : null);
  const out = el("div", { class: "stage-chain" });
  const d = el("details", { class: `stage ${st.kind === "tx" && !st.t.ok ? "refused" : ""}`, "data-kind": st.kind },
    el("summary", {}, el("span", { class: "snum", text: String(i + 1) }), el("span", { class: "stitle", text: head }), pill),
    body, out);
  // Chain unreachable: the recording stands. Show it as recorded, with its explorer link (no alarm badge).
  d._recorded = () => {
    pill.className = "vpill rec";
    pill.textContent = st.kind === "nokey" ? "refused (as expected)" : "recorded ✓";
    out.replaceChildren();
  };
  d._verify = async () => {
    if (st.kind === "nokey") {
      pill.className = "vpill ok";
      pill.textContent = "refused (as expected)";
      out.replaceChildren(el("p", { class: "muted small", text: "Nothing reached the chain: her key was deleted in the sale, so the node refused it before any transaction existed." }));
      return "nokey";
    }
    pill.className = "vpill wait";
    pill.textContent = "checking chain…";
    try {
      const v = st.kind === "scan" ? await verifyScan(st.scan) : await verifyTx(st.t);
      // An attack the contract refused is shown red: the chain confirms the refusal, not a success.
      const refusedTx = st.kind === "tx" && !st.t.ok;
      pill.className = `vpill ${v.matches ? (refusedTx ? "refused" : "ok") : "bad"}`;
      pill.textContent = v.matches ? (refusedTx ? "✗ refused, confirmed on chain" : "✓ confirmed on chain") : "✗ differs from the recording";
      out.replaceChildren(el("p", { class: "chain-title", text: "What NEAR testnet answers now:" }),
        el("dl", { class: "facts-grid" }, v.rows.map(([k, val]) => [el("dt", { text: k }), el("dd", { text: String(val ?? "?") })])));
      return v.matches;
    } catch (e) {
      pill.className = "vpill na";
      if (e.notFound) {
        // The node answered: it does not know this hash (too old for a
        // non-archival node, or a wrong recording). Say so; never "unreachable".
        pill.textContent = "not found on chain";
        out.replaceChildren(el("p", { class: "muted small", text: "NEAR testnet answered that it does not know this transaction. Non-archival nodes keep about 2.5 days; open the NearBlocks link to check by hand." }));
        return null;
      }
      d._recorded();
      return "down";
    }
  };
  return d;
}

function scenarioCard(sc, data) {
  const stages = stagesFor(data, sc.steps);
  const list = el("div", { class: "stages" });
  const summary = el("p", { class: "run-summary muted small" });
  const n = (k) => `${k} stage${k === 1 ? "" : "s"}`;
  const run = el("button", { type: "button", class: "primary", "data-run": sc.id, text: `Run and verify ${n(stages.length)}` });
  run.addEventListener("click", async () => {
    run.disabled = true;
    list.replaceChildren();
    summary.className = "run-summary muted small";
    let ok = 0, bad = 0, na = 0, nokey = 0, refusedTx = 0;
    for (let i = 0; i < stages.length; i++) {
      const d = stageView(stages[i], i);
      list.append(d);
      d.open = true;
      const r = await d._verify();
      if (r === "down") {
        // One banner, not a wall of warnings: show the rest as recorded, each with its tx link.
        d.open = false;
        if (i > 0) list.children[i - 1].open = false;
        for (let j = i + 1; j < stages.length; j++) { const e = stageView(stages[j], j); e._recorded(); list.append(e); }
        summary.className = "run-summary banner small";
        summary.textContent = "Could not reach NEAR testnet from here. Showing the recorded results: open any stage for its transaction on NearBlocks.";
        break;
      }
      if (r === true) { ok++; if (stages[i].kind === "tx" && !stages[i].t.ok) refusedTx++; } else if (r === false) bad++; else if (r === "nokey") nokey++; else na++;
      if (i > 0) list.children[i - 1].open = false;
      summary.textContent = `${ok + nokey} of ${n(stages.length)} match the chain${refusedTx || nokey ? `: ${[refusedTx ? `${refusedTx} refused by the contract` : "", nokey ? `${nokey} rejected at the node (deleted keys)` : ""].filter(Boolean).join(", ")}, as expected` : ""}${bad ? `, ${bad} DIFFER from the recording` : ""}${na ? `, ${na} not found` : ""}.`;
    }
    run.disabled = false;
    run.textContent = "Run again";
  });
  const preview = el("ol", { class: "stage-preview" }, stages.map((st) => el("li", { text: plain(st.kind === "scan" ? `scan of ${st.scan.agent}` : st.t.label) })));
  list.append(preview);
  return el("details", { class: "scenario", "data-scenario": sc.id },
    el("summary", {}, el("b", { text: sc.title }), el("span", { class: "muted small", text: ` · ${n(stages.length)}` })),
    el("p", { text: sc.intro }),
    el("div", { class: "run-row" }, run, summary),
    list);
}

async function main() {
  const box = document.getElementById("scenarios");
  if (!box) return;
  const r = await fetch("data/testnet.json");
  if (!r.ok) return;
  const data = await r.json();
  // A scenario shows only once its testnet scene is recorded: never an empty card.
  // The three that carry the story stay open to view; the rest fold under "N more scenarios".
  const FIRST = ["sale", "cheat", "invest"];
  const shown = SCENARIOS.filter((sc) => stagesFor(data, sc.steps).length > 0);
  const first = FIRST.map((id) => shown.find((sc) => sc.id === id)).filter(Boolean);
  const rest = shown.filter((sc) => !FIRST.includes(sc.id));
  box.replaceChildren(...first.map((sc) => scenarioCard(sc, data)),
    rest.length ? el("details", { class: "sc-more" }, el("summary", { text: `${rest.length} more scenario${rest.length === 1 ? "" : "s"}` }),
      el("div", { class: "scenarios" }, rest.map((sc) => scenarioCard(sc, data)))) : null);
  box.dataset.ready = "1";
  // Review 5: a link to #run-<id> (the hero button) opens that scenario and runs it.
  const autorun = () => {
    const m = /^#run-([a-z]+)$/.exec(location.hash);
    if (!m) return;
    const card = box.querySelector(`details.scenario[data-scenario="${m[1]}"]`);
    const btn = card && card.querySelector("button[data-run]");
    if (!btn || btn.disabled) return;
    const more = card.closest(".sc-more");
    if (more) more.open = true;
    card.open = true;
    // Land on the title (the summary has scroll-margin-top for the sticky nav), not the body.
    // Sections above (Exchange, Invest) appear when their data loads and push the card down: settle again after.
    const sum = card.querySelector(":scope > summary");
    sum.scrollIntoView({ block: "start" });
    for (const ms of [300, 900]) setTimeout(() => { const y = sum.getBoundingClientRect().top; if (y < 40 || y > 120) sum.scrollIntoView({ block: "start" }); }, ms);
    btn.click();
  };
  window.addEventListener("hashchange", autorun);
  autorun();
}

main().catch((e) => {
  const box = document.getElementById("scenarios");
  if (box) box.replaceChildren(el("p", { class: "err", text: `Could not load the scenarios: ${e.message}` }));
});
