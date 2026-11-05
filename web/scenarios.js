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
  { id: "sale", title: "Sell an agent, lock the seller out", steps: ["honest-sale", "break-in"],
    intro: "alice lists translator, bob buys it in one transaction, then alice tries every way back in." },
  { id: "cheat", title: "A cheat is caught before the sale", steps: ["cheat"],
    intro: "alice lists an agent that carries a hidden full-access key. bob's scan reads the account's keys and refuses to buy." },
  { id: "business", title: "The agent runs inside its limits", steps: ["business"],
    intro: "bob, the new owner, sets a daily spending cap. Customers pay the agent; the agent pays a supplier; an over-cap payment is refused." },
  { id: "agent", title: "An AI agent buys an agent", steps: ["agent-buys-agent"],
    intro: "summarizer, itself an agent account, scans translator and buys it from bob through the same contract." },
  { id: "invest", title: "Invest in an agent: 20% of its earnings", steps: ["invest"],
    intro: "alice offers 20% of translator3's earnings for a year; dave scans it and buys. Five customers pay for real NEAR AI translations, each signature checked by the contract; the owner's own sub-account pays too and is flagged. dave gets exactly 20% of every payment and claims it." },
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
  return (m ? m[1] : s).slice(0, 140);
}

/** Re-checks one recorded transaction against the chain; returns rows and a verdict. */
async function verifyTx(t) {
  const res = await rpc("tx", { tx_hash: t.tx, sender_account_id: t.by, wait_until: "NONE" });
  const tx = res.transaction || {};
  const call = (tx.actions || []).map((a) => a.FunctionCall && a.FunctionCall.method_name).find(Boolean) || "(no call)";
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

function stageView(st, i) {
  const head = st.kind === "scan"
    ? `buyer's scan of ${st.scan.agent}: ${st.scan.verdict}`
    : st.t.label;
  const recorded = st.kind === "scan"
    ? `${st.scan.findings.map((f) => `${f.code}: ${f.detail}`).join("; ") || "no findings"}. A read of the account, not a transaction.${st.scan.verdict === "clean" ? "" : " No buy was sent."}`
    : st.kind === "nokey"
      ? `Refused before any transaction existed: ${st.t.error || "the node rejected the key"}.`
      : `${st.t.ok ? "accepted" : `refused: ${st.t.error}`}${st.t.events.length ? `; events ${st.t.events.join(", ")}` : ""}`;
  const pill = el("span", { class: "vpill wait", text: "recorded" });
  const body = el("div", { class: "stage-body" },
    el("p", { class: "stage-rec" }, el("b", { text: "Recorded: " }), recorded),
    st.kind === "tx" ? el("p", { class: "stage-call" }, el("code", { text: `${st.t.by} → ${st.t.receiver}.${st.t.method}()` }), " · ",
      el("a", { href: st.t.explorer, target: "_blank", rel: "noopener noreferrer", text: "NearBlocks" })) : null,
    // Review 5: the check above goes through this page's relay; this one does not.
    st.kind === "tx" ? el("details", { class: "selfcheck" }, el("summary", { text: "Check it without this page" }),
      el("p", { class: "muted small", text: "Ask a public NEAR archival node yourself:" }),
      el("pre", {}, el("code", { text: selfCheck(st.t) }))) : null);
  const out = el("div", { class: "stage-chain" });
  const d = el("details", { class: `stage ${st.kind === "tx" && !st.t.ok ? "refused" : ""}`, "data-kind": st.kind },
    el("summary", {}, el("span", { class: "snum", text: String(i + 1) }), el("span", { class: "stitle", text: head }), pill),
    body, out);
  d._verify = async () => {
    if (st.kind === "nokey") {
      pill.className = "vpill na";
      pill.textContent = "no transaction";
      out.replaceChildren(el("p", { class: "muted small", text: "Nothing reached the chain, so there is nothing to look up: the node refused the deleted key. The live key list (Read live) shows the key is gone." }));
      return "nokey";
    }
    pill.className = "vpill wait";
    pill.textContent = "checking chain…";
    try {
      const v = st.kind === "scan" ? await verifyScan(st.scan) : await verifyTx(st.t);
      pill.className = `vpill ${v.matches ? "ok" : "bad"}`;
      pill.textContent = v.matches ? "✓ confirmed on chain" : "✗ differs from the recording";
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
      } else {
        pill.textContent = "chain unreachable";
        out.replaceChildren(el("p", { class: "muted small", text: `Could not reach NEAR testnet from here (${e.message}). The recording and the NearBlocks link above still stand; open the link to check by hand.` }));
      }
      return null;
    }
  };
  return d;
}

function scenarioCard(sc, data) {
  const stages = stagesFor(data, sc.steps);
  const list = el("div", { class: "stages" });
  const summary = el("p", { class: "run-summary muted small" });
  const run = el("button", { type: "button", class: "primary", "data-run": sc.id, text: `Run and verify ${stages.length} stages` });
  run.addEventListener("click", async () => {
    run.disabled = true;
    list.replaceChildren();
    let ok = 0, bad = 0, na = 0, nokey = 0;
    for (let i = 0; i < stages.length; i++) {
      const d = stageView(stages[i], i);
      list.append(d);
      d.open = true;
      const r = await d._verify();
      if (r === true) ok++; else if (r === false) bad++; else if (r === "nokey") nokey++; else na++;
      if (i > 0) list.children[i - 1].open = false;
      summary.textContent = `${ok} of ${stages.length} stages confirmed on chain${nokey ? `, ${nokey} refused by the node before any transaction` : ""}${bad ? `, ${bad} DIFFER from the recording` : ""}${na ? `, ${na} could not be checked` : ""}.`;
    }
    run.disabled = false;
    run.textContent = "Run again";
  });
  const preview = el("ol", { class: "stage-preview" }, stages.map((st) => el("li", { text: st.kind === "scan" ? `scan of ${st.scan.agent}` : st.t.label })));
  list.append(preview);
  return el("details", { class: "scenario", "data-scenario": sc.id },
    el("summary", {}, el("b", { text: sc.title }), el("span", { class: "muted small", text: ` · ${stages.length} stages` })),
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
  box.replaceChildren(...SCENARIOS.map((sc) => scenarioCard(sc, data)));
  box.dataset.ready = "1";
  // Review 5: a link to #run-<id> (the hero button) opens that scenario and runs it.
  const autorun = () => {
    const m = /^#run-([a-z]+)$/.exec(location.hash);
    if (!m) return;
    const card = box.querySelector(`details.scenario[data-scenario="${m[1]}"]`);
    const btn = card && card.querySelector("button[data-run]");
    if (!btn || btn.disabled) return;
    card.open = true;
    card.scrollIntoView({ behavior: "smooth", block: "start" });
    btn.click();
  };
  window.addEventListener("hashchange", autorun);
  autorun();
}

main().catch((e) => {
  const box = document.getElementById("scenarios");
  if (box) box.replaceChildren(el("p", { class: "err", text: `Could not load the scenarios: ${e.message}` }));
});
