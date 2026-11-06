//! `make business-loop`: 30 days of an autonomous agent business, offline,
//! through the real contract code (the same `sim` the tests use).
//!
//! Humans act only at the start: bob (the owner who bought `translator`)
//! sets the limits and offers 10% of earnings; dave buys the slice. Then for
//! 30 days nobody touches the agent. Customers pay; every payment is split by
//! the contract (dave's 10% first, then owner 80% / treasury 20%); the
//! runner key buys work from another agent (`summarizer`) out of the
//! treasury, inside the daily cap and only to the allowed payee; the old
//! seller's earn-out is released from counted payments. Refusals are part of
//! the run: an over-cap spend and a payment to a payee that is not allowed.
//!
//! Writes `out/business-loop.json`; `--golden` compares with
//! `tests/golden/business-loop.json`. All money is xUSD-FAKE.
use agent_exit::sim::*;
use near_sdk::json_types::{U128, U64};
use serde_json::{json, Value};
use std::panic::{catch_unwind, AssertUnwindSafe};

const DAVE: &str = "dave.testnet";
const API_JOB: u128 = 2 * USD; // what summarizer charges per job

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
        Some((_, rest)) => rest
            .rsplit_once("\" }")
            .map(|x| x.0)
            .unwrap_or(rest)
            .to_string(),
        None => raw,
    }
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let arg = |k: &str| {
        args.iter()
            .position(|a| a == k)
            .and_then(|i| args.get(i + 1).cloned())
    };
    let out = arg("--out").unwrap_or_else(|| "out/business-loop.json".into());
    // Refusals are expected; their panics are read, not printed.
    std::panic::set_hook(Box::new(|_| {}));

    // ---- the humans set the constraints, once --------------------------
    let mut s = sold_translator();
    let start = s.now;
    let mut l = demo_limits(); // cap 50.00/day, payee summarizer, split 80/20
    l.daily_cap = U128(8 * USD);
    s.by_1y(BOB).set_limits(l);
    let end = U64(s.now + 365 * DAY);
    let o = s.by_1y(BOB).offer_share(1_000, U128(200 * USD), end);
    s.register(DAVE);
    let msg = json!({"kind": "buy_share", "offer_id": o["offer_id"], "bps": o["bps"],
                     "price": o["price"], "end": o["end"], "part": 1_000})
    .to_string();
    assert_eq!(s.ft(DAVE, 200 * USD, &msg, 100), Some(0), "dave buys 10%");
    let human_calls = 3; // set_limits, offer_share, dave's buy

    // ---- 30 days, no human --------------------------------------------------
    let mut days = Vec::new();
    let (mut revenue, mut supplier_paid, mut jobs_bought) = (0u128, 0u128, 0u32);
    let mut refusals: Vec<Value> = Vec::new();
    for day in 0..30u64 {
        s.now = start + (day + 1) * DAY + 3_600 * agent_core::NANOS_PER_SEC;
        // Customers: 2 to 4 a day, 10.00 to 25.00 each (fixed pattern).
        let n = 2 + (day % 3) as usize;
        let mut paid_today = 0;
        for i in 0..n {
            let amount = (10 + ((day * 7 + i as u64 * 5) % 16) as u128) * USD;
            let payer = PAYERS[(day as usize + i) % PAYERS.len()];
            let job = format!(r#"{{"kind":"pay","job_id":"loop-{day}-{i}"}}"#);
            assert_eq!(s.ft(payer, amount, &job, 100), Some(0));
            paid_today += amount;
        }
        revenue += paid_today;
        // The runner buys summarizer jobs while the treasury and the cap allow.
        let mut bought = 0;
        loop {
            let r = catch_unwind(AssertUnwindSafe(|| {
                s.runner(key(BOB_RUNNER))
                    .pay(acc(SUMMARIZER), U128(API_JOB))
            }));
            match r {
                Ok(_) => {
                    bought += 1;
                    supplier_paid += API_JOB;
                }
                Err(e) => {
                    let why = panic_text(e);
                    let parts = if why.starts_with("PolicyViolation") {
                        2
                    } else {
                        1
                    };
                    let kind = why.split(':').take(parts).collect::<Vec<_>>().join(":");
                    if !refusals.iter().any(|r| r["kind"] == json!(kind)) {
                        refusals.push(json!({"day": day + 1, "kind": kind,
                            "what": format!("summarizer job {} of the day", bought + 1), "error": why}));
                    }
                    break;
                }
            }
        }
        jobs_bought += bought;
        if day == 9 {
            let r = catch_unwind(AssertUnwindSafe(|| {
                s.runner(key(BOB_RUNNER)).pay(acc(MALLORY), U128(USD))
            }));
            let why = r
                .err()
                .map(panic_text)
                .unwrap_or_else(|| "NOT REFUSED".into());
            refusals.push(json!({"day": day + 1, "kind": "PolicyViolation:Payee", "what": "pay mallory (not an allowed payee)", "error": why}));
        }
        let st = s.c.get_status();
        days.push(
            json!({"day": day + 1, "customers": n, "revenue": usd(paid_today),
                         "summarizer_jobs": bought,
                         "treasury": usd(st["treasury"].as_str().unwrap().parse().unwrap())}),
        );
    }

    // ---- results --------------------------------------------------------------
    let h = s.c.get_holdings(acc(DAVE));
    let dave_earned: u128 = h["claimable"].as_str().unwrap().parse().unwrap();
    let alice_earnout = s.m().sales.iter().map(|x| x.released).sum::<u128>();
    let st = s.c.get_status();
    let checks = json!({
        "dave_got_10_percent": dave_earned == revenue / 10,
        "supplier_paid_within_cap": supplier_paid <= 30 * 8 * USD,
        "refusals_refused": refusals.iter().all(|r| r["error"] != "NOT REFUSED"),
        "daily_cap_held": refusals.iter().any(|r| r["kind"] == "PolicyViolation:DailyCap"),
        "solvent": st["solvent"] == json!(true),
    });
    let pass = checks
        .as_object()
        .unwrap()
        .values()
        .all(|v| v == &json!(true));
    let v = json!({
        "schema": "business-loop.v1",
        "money": "xUSD-FAKE (test token)",
        "agent": AGENT,
        "human_calls_at_start": human_calls,
        "human_calls_during_30_days": 0,
        "customers_revenue": usd(revenue),
        "to_investor_dave": usd(dave_earned),
        "summarizer_jobs_bought": jobs_bought,
        "paid_to_summarizer": usd(supplier_paid),
        "earnout_released_to_old_seller": usd(alice_earnout),
        "treasury_left": usd(st["treasury"].as_str().unwrap().parse().unwrap()),
        "refusals": refusals,
        "checks": checks,
        "pass": pass,
        "days": days,
    });
    let text = serde_json::to_string_pretty(&v).unwrap() + "\n";
    if let Some(dir) = std::path::Path::new(&out).parent() {
        std::fs::create_dir_all(dir).ok();
    }
    std::fs::write(&out, &text).expect("write");
    eprintln!(
        "business-loop: 30 days, 0 human calls; revenue {}, dave {} (10%), {} summarizer jobs ({}), {} refusals; pass={}",
        usd(revenue),
        usd(dave_earned),
        jobs_bought,
        usd(supplier_paid),
        v["refusals"].as_array().unwrap().len(),
        pass
    );
    if let Some(g) = arg("--golden") {
        let want = std::fs::read_to_string(&g).expect("golden");
        if want != text {
            eprintln!("business-loop: output differs from {g}");
            std::process::exit(1);
        }
        eprintln!("business-loop: output equals {g}");
    }
    if !pass {
        std::process::exit(1);
    }
}
