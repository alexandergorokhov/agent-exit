//! `make attack [N=200 SEED=42 CLEAN=50 FIXTURE=fixtures/judge-attack.json]`
//!
//! 1. Generates a seeded corpus of backdoored agents (A-series families) and
//!    clean controls, runs the client scan on each, and counts caught /
//!    missed / false positives.
//! 2. Runs the contract-side refusals (the contract keeps enforcing listing
//!    version, key hash, approvals, token, deed transfer and hidden keys) in
//!    the mocked runtime.
//! 3. Optionally scans a judge's own fixture.
//!
//! Prints a table on stderr and `attack-report.v1` JSON on stdout / `--out`.
//! Exit 1 if anything in scope was missed or a clean agent was blocked, or if
//! `--golden` differs.

#![allow(unused_must_use)]

use agent_core::USD;
use agent_exit::sim::*;
use agent_exit::DEED_ID;
use near_sdk::json_types::U128;
use serde_json::{json, Value};
use std::panic::{catch_unwind, AssertUnwindSafe};

fn arg(args: &[String], f: &str) -> Option<String> {
    args.iter()
        .position(|a| a == f)
        .and_then(|i| args.get(i + 1))
        .cloned()
}

fn panics_with(needle: &str, f: impl FnOnce()) -> bool {
    match catch_unwind(AssertUnwindSafe(f)) {
        Ok(()) => false,
        Err(e) => e
            .downcast_ref::<String>()
            .map(|m| m.contains(needle))
            .or_else(|| e.downcast_ref::<&str>().map(|m| m.contains(needle)))
            .unwrap_or(false),
    }
}

fn refused(reason: &str) -> bool {
    events("refused").iter().any(|e| e["reason"] == reason)
}

/// The contract's own refusals, independent of the client scan.
fn contract_layer() -> Vec<Value> {
    let mut out = vec![];
    let mut row = |id: &str, what: &str, blocked: bool| {
        out.push(json!({"id": id, "what": what, "blocked": blocked}));
    };

    let mut s = listed_translator();
    let (v, h) = (s.m().listing_version, s.c.known_keys_hash());
    s.by(ALICE).cancel_listing();
    s.by(ALICE)
        .list_for_sale(U128(1_500 * USD), CODE_HASH.into(), None);
    let r = s.ft(BOB, 1_500 * USD, &Sim::buy_msg(v, &h, BOB_RUNNER), 300);
    row(
        "A7",
        "buy with the listing version the buyer scanned after the seller changed terms: refused ListingChanged, full refund",
        r == Some(1_500 * USD) && refused("ListingChanged"),
    );

    let mut s = listed_translator();
    let v = s.m().listing_version;
    let r = s.ft(BOB, 1_000 * USD, &Sim::buy_msg(v, "00", BOB_RUNNER), 300);
    row(
        "A2-A4",
        "buy with a known_keys_hash that no longer matches: refused KeysChanged, full refund",
        r == Some(1_000 * USD) && refused("KeysChanged"),
    );

    let mut s = translator_world();
    s.by_1y(ALICE)
        .nft_approve(DEED_ID.into(), acc(MALLORY), None);
    let a8 = panics_with("OpenApproval", || {
        s.by(ALICE)
            .list_for_sale(U128(1_000 * USD), CODE_HASH.into(), None);
    });
    row(
        "A8",
        "list with an open deed approval: refused OpenApproval",
        a8,
    );

    let mut s = translator_world();
    let a14 = panics_with("UnknownToken", || {
        s.by(MALLORY)
            .ft_on_transfer(acc(MALLORY), U128(USD), "".into());
    });
    row("A14", "fake token calls ft_on_transfer: UnknownToken", a14);

    let mut s = translator_world();
    s.by_1y(ALICE)
        .nft_transfer(acc(MALLORY), DEED_ID.into(), None, None);
    row(
        "A16",
        "deed moved outside buy(): runner keys deleted, agent paused",
        key_actions(AGENT) == vec![("delete".to_string(), key_str(ALICE_RUNNER))] && s.m().paused,
    );

    let mut s = sold_translator();
    let r = s.runner(key(HIDDEN_KEY)).heartbeat();
    row(
        "A23",
        "hidden key calls the contract as the agent: frozen, escrow back to buyer",
        r["frozen"] == true && s.c.get_claimable(acc(BOB)).0 == 300 * USD,
    );
    out
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let n: usize = arg(&args, "--n")
        .and_then(|v| v.parse().ok())
        .unwrap_or(200);
    let seed: u64 = arg(&args, "--seed")
        .and_then(|v| v.parse().ok())
        .unwrap_or(42);
    let clean_n: usize = arg(&args, "--clean")
        .and_then(|v| v.parse().ok())
        .unwrap_or(50);
    std::panic::set_hook(Box::new(|_| {}));

    let mut report = scan::corpus::run(seed, n, clean_n);
    let layer = contract_layer();
    let layer_ok = layer.iter().all(|r| r["blocked"] == true);
    report["contract_layer"] = json!(layer);
    report["schema"] = json!("attack-report.v1");
    report["n"] = json!(n);

    let judge = arg(&args, "--fixture").map(|path| {
        let text = std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("read {path}: {e}"));
        let input: scan::ScanInput = serde_json::from_str(&text).unwrap_or_else(|e| {
            eprintln!("attack: {path} is not scan-input.v1: {e}");
            std::process::exit(1)
        });
        json!({"fixture": path, "report": scan::scan(&input)})
    });
    report["judge"] = judge.clone().unwrap_or(Value::Null);
    let _ = std::panic::take_hook();

    // Human table on stderr.
    eprintln!(
        "{:6} {:62} {:>5} {:>6} {:>6}",
        "id", "attack", "cases", "caught", "missed"
    );
    for f in report["families"].as_array().unwrap() {
        eprintln!(
            "{:6} {:62} {:>5} {:>6} {:>6}",
            f["id"].as_str().unwrap(),
            f["attack"].as_str().unwrap(),
            f["cases"],
            f["caught"],
            f["missed"]
        );
    }
    let ins = &report["in_scope"];
    let cl = &report["clean"];
    eprintln!(
        "in scope: {}/{} blocked; clean agents blocked (false positives): {}/{}",
        ins["caught"], ins["cases"], cl["false_positives"], cl["cases"]
    );
    for r in report["contract_layer"].as_array().unwrap() {
        eprintln!(
            "contract {:6} {} {}",
            r["id"].as_str().unwrap(),
            if r["blocked"] == true {
                "BLOCKED"
            } else {
                "NOT BLOCKED"
            },
            r["what"].as_str().unwrap()
        );
    }
    for x in report["not_caught_by_design"].as_array().unwrap() {
        eprintln!(
            "not caught by design {}: {} ({})",
            x["id"].as_str().unwrap(),
            x["not_caught"].as_str().unwrap(),
            x["why"].as_str().unwrap()
        );
    }
    if let Some(j) = &judge {
        let r = &j["report"];
        eprintln!(
            "judge fixture {}: {} {:?}",
            j["fixture"].as_str().unwrap(),
            r["verdict"].as_str().unwrap(),
            r["findings"]
                .as_array()
                .unwrap()
                .iter()
                .map(|f| f["code"].as_str().unwrap_or(""))
                .collect::<Vec<_>>()
        );
    }

    let text = serde_json::to_string_pretty(&report).expect("json") + "\n";
    print!("{text}");
    if let Some(p) = arg(&args, "--out") {
        std::fs::write(&p, &text).expect("write --out");
        eprintln!("attack: wrote {p}");
    }
    let mut ok = ins["missed"] == 0 && cl["false_positives"] == 0 && layer_ok;
    if !ok {
        eprintln!("attack: something in scope was missed or a clean agent was blocked");
    }
    if let Some(g) = arg(&args, "--golden") {
        let golden: Value = serde_json::from_str(&std::fs::read_to_string(&g).expect("golden"))
            .expect("golden json");
        if golden == report {
            eprintln!("attack: output equals {g}");
        } else {
            eprintln!("attack: output differs from {g}");
            ok = false;
        }
    }
    std::process::exit(if ok { 0 } else { 1 });
}
