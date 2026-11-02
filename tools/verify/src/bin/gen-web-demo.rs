//! Writes `web/data/demo.json`: what the hosted demo page shows offline.
//!
//! Every call, error text, event and number in it is RECORDED from the real
//! agent contract in near-sdk's mocked runtime (the simulator `make verify`
//! uses), from the real scanner (`tools/scan`) and from the recorded MCP run
//! (`tests/golden/agent-buys-agent.json`). The page only displays it. Money
//! is xUSD-FAKE, a fake test dollar.
//!
//! Steps follow the demo order: break-in fails; the business runs inside
//! its limits and an over-limit payment fails; verified vs unverified
//! earnings; a cheat caught before the sale; an honest sale in one step; an
//! agent buys an agent.
//!
//! `gen-web-demo --check` fails if the committed file differs.

#![allow(unused_must_use)]

use agent_exit::sim::*;
use agent_exit::{OutKind, DEED_ID};
use near_sdk::json_types::U128;
use serde_json::{json, Value};
use std::panic::{catch_unwind, AssertUnwindSafe};

const ROOT: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../..");

fn usd(a: u128) -> String {
    format!("{}.{:02}", a / USD, (a % USD) / 10_000)
}

fn panic_text(e: Box<dyn std::any::Any + Send>) -> String {
    let raw = e
        .downcast_ref::<String>()
        .cloned()
        .or_else(|| e.downcast_ref::<&str>().map(|s| s.to_string()))
        .unwrap_or_default();
    match raw.split_once("panic_msg: \"") {
        Some((_, rest)) => rest.rsplit_once("\" }").map(|x| x.0).unwrap_or(rest).into(),
        None => raw,
    }
}

/// Contract events (NEP-297 `EVENT_JSON:` logs) of the last call.
fn call_events() -> Vec<Value> {
    logs()
        .iter()
        .filter_map(|l| l.strip_prefix("EVENT_JSON:"))
        .filter_map(|j| serde_json::from_str::<Value>(j).ok())
        .collect()
}

/// One contract call as the page shows it: who, what, and exactly what the
/// contract answered (return value or panic text), its events and the
/// receipts it scheduled (key actions, token transfers).
fn rec(by: &str, call: &str, f: impl FnOnce() -> Value) -> Value {
    let r = catch_unwind(AssertUnwindSafe(f));
    let events = call_events();
    let keys: Vec<Value> = key_actions(AGENT)
        .into_iter()
        .map(|(a, k)| json!({"action": a, "key": k}))
        .collect();
    let transfers: Vec<Value> = ft_transfers()
        .into_iter()
        .map(|(to, a)| json!({"to": to, "amount": usd(a)}))
        .collect();
    let mut v = json!({"by": by, "call": call});
    match r {
        Ok(result) => {
            v["outcome"] = json!("ok");
            if !result.is_null() {
                v["result"] = result;
            }
        }
        Err(e) => {
            v["outcome"] = json!("error");
            v["error"] = json!(panic_text(e));
        }
    }
    if !events.is_empty() {
        v["events"] = json!(events);
    }
    if !keys.is_empty() {
        v["key_actions"] = json!(keys);
    }
    if !transfers.is_empty() {
        v["token_transfers"] = json!(transfers);
    }
    v
}

fn nothing<T>(_: T) -> Value {
    Value::Null
}

fn break_in() -> Value {
    let mut s = sold_translator();
    let mut calls = vec![];
    calls.push(rec(ALICE, "withdraw({amount: 1.00})", || {
        nothing(s.by_1y(ALICE).withdraw(U128(USD), None))
    }));
    calls.push(rec(ALICE, "set_runner_key(alice's new key)", || {
        nothing(s.by_1y(ALICE).set_runner_key(key(9)))
    }));
    calls.push(rec(ALICE, "list_for_sale({price: 1.00})", || {
        s.by_1y(ALICE).list_for_sale(U128(USD), "x".into(), None)
    }));
    calls.push(rec(ALICE, "set_limits({...})", || {
        s.by_1y(ALICE).set_limits(demo_limits())
    }));
    calls.push(rec(ALICE, "nft_transfer({receiver: alice})", || {
        s.by_1y(ALICE)
            .nft_transfer(acc(ALICE), DEED_ID.into(), None, None);
        Value::Null
    }));
    // A planted key: a self-call signed by a key the contract never added.
    let mut h = sold_translator();
    let planted = rec("translator.testnet (hidden key)", "heartbeat()", || {
        h.runner(key(HIDDEN_KEY)).heartbeat()
    });
    let refunded = *h.m().claimable.get(BOB).unwrap_or(&0);
    json!({
        "id": "break-in",
        "title": "Break-in fails",
        "scenarios": ["A16", "A17", "A19", "A23", "A25"],
        "story": "translator was sold to bob. alice, the old seller, tries every way back in. Each owner method refuses her with NotOwner. Her runner key was deleted in the sale's key batch, so on a real node a transaction signed with it is rejected before it reaches the contract (AccessKeyNotFound, sandbox test S5/A17). A key the contract never added (a planted backdoor) that calls the agent freezes it and returns the open escrow to the buyer.",
        "headline": [
            {"label": "old seller's calls refused", "value": format!("{}/{}", calls.iter().filter(|c| c["outcome"] == "error").count(), calls.len())},
            {"label": "escrow back to buyer after the planted key", "value": usd(refunded)},
        ],
        "calls": calls.into_iter().chain([planted]).collect::<Vec<_>>(),
    })
}

fn business() -> Value {
    let mut s = sold_translator();
    let mut calls = vec![];
    calls.push(rec(
        BOB,
        "set_limits(price floor 5.00, daily cap 50.00, payee summarizer, split 80/20)",
        || s.by_1y(BOB).set_limits(demo_limits()),
    ));
    s.advance(3_600 * agent_core::NANOS_PER_SEC);
    for (p, job) in [(PAYERS[0], "job-b1"), (PAYERS[1], "job-b2")] {
        let msg = json!({"kind": "pay", "job_id": job}).to_string();
        calls.push(rec(
            p,
            "ft_transfer_call(agent, 25.00) → pay",
            || json!({"unused": usd(s.ft(p, 25 * USD, &msg, 100).unwrap_or(0))}),
        ));
    }
    calls.push(rec(
        "translator.testnet (runner key)",
        "pay(summarizer, 8.00)",
        || {
            s.runner(key(BOB_RUNNER))
                .pay(acc(SUMMARIZER), U128(8 * USD))
        },
    ));
    s.callback(true)
        .on_transfer_out(OutKind::TreasuryPay, acc(SUMMARIZER), U128(8 * USD), None);
    calls.push(rec(
        "translator.testnet (runner key)",
        "pay(summarizer, 45.00)",
        || {
            s.runner(key(BOB_RUNNER))
                .pay(acc(SUMMARIZER), U128(45 * USD))
        },
    ));
    calls.push(rec(
        "translator.testnet (runner key)",
        "pay(mallory, 1.00)",
        || s.runner(key(BOB_RUNNER)).pay(acc(MALLORY), U128(USD)),
    ));
    calls.push(rec(ALICE, "set_limits({...}) as the old seller", || {
        s.by_1y(ALICE).set_limits(demo_limits())
    }));
    let limits = s.by(MALLORY).get_limits();
    json!({
        "id": "business",
        "title": "The business runs inside its limits",
        "scenarios": ["S11", "S12", "S13"],
        "story": "bob, the new owner, has one business method: set_limits. Every incoming payment is split automatically (owner 80%, treasury 20%). The runner pays another agent, summarizer, from the treasury. 8.00 is inside the daily cap; 45.00 more would pass it, so the contract refuses and nothing moves. A payee that is not allowed is refused too, and only the owner can change the limits.",
        "headline": [
            {"label": "spent today", "value": usd(limits["spent_today"].as_str().unwrap_or("0").parse().unwrap_or(0))},
            {"label": "daily cap", "value": usd(limits["daily_cap"].as_str().unwrap_or("0").parse().unwrap_or(0))},
            {"label": "treasury left", "value": usd(limits["treasury"].as_str().unwrap_or("0").parse().unwrap_or(0))},
        ],
        "calls": calls,
        "view": {"name": "get_limits()", "value": limits},
    })
}

fn verified() -> Value {
    let mut s = Sim::new(AGENT, ALICE);
    let mut calls = vec![];
    calls.push(rec(
        PAYERS[0],
        "ft_transfer_call(agent, 12.00) → pay with a NEAR AI signature",
        || json!({"unused": usd(s.ft(PAYERS[0], 12 * USD, &job_msg(&job(0)), 100).unwrap_or(0))}),
    ));
    calls.push(rec(PAYERS[1], "ft_transfer_call(agent, 12.00) → pay, no signature", || {
        json!({"unused": usd(s.ft(PAYERS[1], 12 * USD, r#"{"kind":"pay","job_id":"job-2"}"#, 100).unwrap_or(0))})
    }));
    calls.push(rec(
        PAYERS[0],
        "the same signature again",
        || json!({"unused": usd(s.ft(PAYERS[0], 12 * USD, &job_msg(&job(0)), 100).unwrap_or(0))}),
    ));
    let mut t = translator_world();
    let e = serde_json::to_value(t.by(MALLORY).get_earnings()).expect("json");
    let n = |k: &str| e[k].as_u64().unwrap_or(0) as u128;
    json!({
        "id": "verified",
        "title": "Verified vs unverified earnings",
        "scenarios": ["S2", "S3", "E11"],
        "story": "Every payment is counted by payer and day. A payment that carries a NEAR AI Cloud signature over its request and response, naming this payer and amount, is checked on-chain (ecrecover) and marked verified; without one it still counts, unverified. A signature works once. Payments by the owner, past owners or their sub-accounts, and round trips, are shown but never counted. (Signatures here are made with local TEST keys.)",
        "headline": [
            {"label": "gross", "value": usd(n("gross"))},
            {"label": "counted (real customers)", "value": usd(n("counted"))},
            {"label": "verified (NEAR AI signature)", "value": usd(n("verified"))},
            {"label": "flagged (self, sub-accounts)", "value": usd(n("flagged"))},
        ],
        "calls": calls,
        "view": {"name": "translator: get_earnings()", "value": e},
    })
}

fn cheat() -> Value {
    let mut c = cheat_world();
    let e = serde_json::to_value(c.by(MALLORY).get_earnings()).expect("json");
    let input: scan::ScanInput = serde_json::from_str(
        &std::fs::read_to_string(format!("{ROOT}/fixtures/scan/cheat.json"))
            .expect("cheat fixture"),
    )
    .expect("scan-input.v1");
    let report = scan::scan(&input);
    let n = |k: &str| e[k].as_u64().unwrap_or(0) as u128;
    json!({
        "id": "cheat",
        "title": "A cheat is caught before the sale",
        "scenarios": ["S1", "S4", "A1"],
        "story": "cheat claims 500.00 of revenue, all of it paid by alice and her own sub-accounts: the meter counts none of it. The account also has a hidden full-access key that the contract never added. The buyer's scan reads the key list from two RPC providers and blocks the sale; the buy tool refuses before any transaction is sent.",
        "headline": [
            {"label": "claimed gross", "value": usd(n("gross"))},
            {"label": "counted", "value": usd(n("counted"))},
            {"label": "scan verdict", "value": report.verdict.clone()},
        ],
        "calls": [],
        "scan": serde_json::to_value(&report).expect("json"),
        "view": {"name": "cheat: get_earnings()", "value": e},
    })
}

fn honest_sale() -> Value {
    let mut s = listed_translator();
    let listing = s.by(MALLORY).get_listing().unwrap_or(Value::Null);
    let mut calls = vec![];
    let v = s.m().listing_version;
    let h = s.c.known_keys_hash();
    calls.push(rec(
        BOB,
        "ft_transfer_call(agent, 1,000.00) → buy",
        || {
            let msg = Sim::buy_msg(v, &h, BOB_RUNNER);
            json!({"promise": s.ft(BOB, 1_000 * USD, &msg, 300).is_none()})
        },
    ));
    calls.push(rec(
        "translator.testnet (callback)",
        "on_handover(sale 1) after the key batch",
        || json!({"unused": usd(s.callback(true).on_handover(1).0)}),
    ));
    let status = s.by(MALLORY).get_status();
    json!({
        "id": "honest-sale",
        "title": "An honest sale in one step",
        "scenarios": ["S5", "S6", "S7", "A17"],
        "story": "bob pays 1,000.00 in one token transfer that names the listing version and key hash his scan saw. The contract checks both, then in one all-or-nothing key batch deletes alice's runner key and adds bob's. After the batch the deed moves to bob, alice is paid 700.00 now and 300.00 stays in escrow, released to alice as customers keep paying (earn-out).",
        "headline": [
            {"label": "price", "value": "1000.00"},
            {"label": "paid now", "value": "700.00"},
            {"label": "escrow (earn-out)", "value": usd(s.m().escrow_left())},
            {"label": "owner after", "value": s.m().owner.clone()},
        ],
        "calls": calls,
        "listing": listing,
        "view": {"name": "get_status() after", "value": status},
    })
}

fn agent_buys_agent() -> Value {
    let g: Value = serde_json::from_str(
        &std::fs::read_to_string(format!("{ROOT}/tests/golden/agent-buys-agent.json"))
            .expect("agent-buys-agent golden"),
    )
    .expect("json");
    let steps: Vec<Value> = g["steps"]
        .as_array()
        .expect("steps")
        .iter()
        .map(|st| {
            let r = &st["response"];
            let mut v =
                json!({"why": st["why"], "tool": st["tool"], "args": st["args"], "ok": st["ok"]});
            if st["ok"] == true {
                v["response"] = r.clone();
            } else {
                v["error"] = r["error"].clone();
            }
            v
        })
        .collect();
    json!({
        "id": "agent-buys-agent",
        "title": "An agent buys an agent",
        "scenarios": ["S14"],
        "story": "The summarizer agent shops through the Agent Exit MCP server: it lists agents, checks earnings, scans, refuses the cheat, buys translator inside its max price, checks that the old seller is locked out, and lists translator again. Each response is the MCP tool's JSON (offline mode replays results recorded from the contract).",
        "headline": [
            {"label": "bought", "value": g["summary"]["bought"].to_string()},
            {"label": "cheat refused", "value": g["summary"]["cheat_buy_refused"].to_string()},
            {"label": "old seller locked out", "value": g["summary"]["seller_locked_out_after"].to_string()},
        ],
        "mcp_steps": steps,
    })
}

fn build() -> String {
    let doc = json!({
        "schema": "web-demo.v1",
        "project": "Agent Exit",
        "money": "xUSD-FAKE, a fake test dollar with no value (6 decimals)",
        "source": "recorded from the agent contract in near-sdk's mocked runtime, tools/scan and the MCP run (tools/verify/src/bin/gen-web-demo.rs)",
        "steps": [break_in(), business(), verified(), cheat(), honest_sale(), agent_buys_agent()],
    });
    serde_json::to_string_pretty(&doc).expect("json") + "\n"
}

fn main() {
    std::panic::set_hook(Box::new(|_| {}));
    let path = format!("{ROOT}/web/data/demo.json");
    let text = build();
    let _ = std::panic::take_hook();
    if std::env::args().any(|a| a == "--check") {
        let have = std::fs::read_to_string(&path).unwrap_or_default();
        if have != text {
            eprintln!("{path} differs from what the generator makes (run make web-data)");
            std::process::exit(1);
        }
        eprintln!("{path} is reproducible");
    } else {
        std::fs::create_dir_all(format!("{ROOT}/web/data")).expect("mkdir");
        std::fs::write(&path, text).expect("write");
        eprintln!("wrote {path}");
    }
}
