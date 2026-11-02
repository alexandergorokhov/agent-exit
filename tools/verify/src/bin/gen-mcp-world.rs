//! Writes `fixtures/mcp/world.json`: what the MCP server's offline mode reads.
//!
//! Every number and every result in it is RECORDED from the real contract
//! code in near-sdk's mocked runtime (the same simulator `make verify` uses),
//! so the offline MCP answers are what the contract answers, not a second
//! implementation. Money is xUSD-FAKE.
//!
//! - two listed agents: `translator` (honest) and `cheat` (fake earnings and
//!   a hidden full-access key on the account);
//! - one recorded buy: `summarizer` buys `translator` at 1,000.00 with the
//!   listing version and key hash its scan saw, plus the after-state and the
//!   old seller's refused calls (lockout probes);
//! - one recorded resale listing: `summarizer` lists `translator` at 1,200.00.
//!
//! `gen-mcp-world --check` fails if the committed file differs.

#![allow(unused_must_use)]

use agent_exit::sim::*;
use agent_exit::{OutKind, RUNNER_ALLOWANCE, RUNNER_METHODS};
use near_sdk::json_types::U128;
use serde_json::{json, Value};
use std::panic::{catch_unwind, AssertUnwindSafe};

/// The published release hash the offline scan compares with (same value as
/// the scan corpus; the reproducible build hash replaces it later).
const FIXTURE_CODE_HASH: &str = "fixture-release-hash-v1";
pub const SUMMARIZER_RUNNER: u8 = 4;

fn refusal(f: impl FnOnce()) -> String {
    match catch_unwind(AssertUnwindSafe(f)) {
        Ok(()) => "accepted".into(),
        Err(e) => {
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
    }
}

fn fc_key(agent: &str, k: &str) -> Value {
    json!({"public_key": k, "permission": {
        "receiver_id": agent,
        "method_names": RUNNER_METHODS.split(',').collect::<Vec<_>>(),
        "allowance": RUNNER_ALLOWANCE.as_yoctonear().to_string(),
    }})
}

/// What a buyer reads before paying, as `scan-input.v1`. `hidden_full` adds a
/// full-access key the contract does not know about (visible only on chain).
fn scan_input(s: &mut Sim, hidden_full: Option<String>) -> Value {
    let agent = s.agent.clone();
    let st = s.by(MALLORY).get_status();
    let l = s.by(MALLORY).get_listing();
    let bw = s.by(MALLORY).get_base_wallet();
    let mut keys: Vec<Value> = s.c.known_keys().iter().map(|k| fc_key(&agent, k)).collect();
    if let Some(k) = hidden_full {
        keys.push(json!({"public_key": k, "permission": "FullAccess"}));
    }
    json!({
        "schema": "scan-input.v1",
        "agent": agent,
        "now_ns": s.now,
        "expected": {"code_hash": FIXTURE_CODE_HASH, "listing_version": st["listing_version"]},
        "chain": {
            "code_hash": FIXTURE_CODE_HASH,
            "methods": scan::PUBLISHED_METHODS.to_vec(),
            "keys": keys,
            "keys_second_provider": keys,
            "subaccounts": [],
        },
        "contract": {
            "status": st["status"], "owner": st["owner"],
            "listing_version": st["listing_version"],
            "known_keys": st["known_keys"], "known_keys_hash": st["known_keys_hash"],
            "open_approvals": st["open_approvals"], "open_auth_until": 0,
            "frozen": st["frozen"],
            "admin": st["admin"], "token": st["token"],
            "paid_to_total": st["paid_to_total"],
            "near_storage_solvent": st["near_storage"]["solvent"],
            // A51: the wallet as the agent reports it, and the TEST MPC
            // signer's root key (the buyer reads it from the signer).
            "base_wallet": {
                "address": bw["address"], "path": bw["path"],
                "mpc_public_key": base_wallet::near_secp_pk(&base_wallet::MockMpc::default().public_key()),
            },
        },
        "listing": l.map(|l| json!({
            "listing_version": l["listing_version"], "price": l["price"], "terms": l["terms"],
        })),
    })
}

/// One agent as the MCP server sees it.
fn snapshot(s: &mut Sim, key_owners: Value, hidden_full: Option<String>) -> Value {
    let scan = scan_input(s, hidden_full.clone());
    let c = s.by(MALLORY);
    json!({
        "status": c.get_status(),
        "earnings": c.get_earnings(),
        "listing": c.get_listing(),
        "limits": c.get_limits(),
        "sales": c.get_sales(None, None),
        "access_keys": scan["chain"]["keys"],
        "key_owners": key_owners,
        "scan_input": scan,
    })
}

fn build() -> String {
    // translator, listed by alice at 1,000.00.
    let mut t = listed_translator();
    let alice_key = key_str(ALICE_RUNNER);
    let translator = snapshot(&mut t, json!({ (alice_key.clone()): ALICE }), None);

    // cheat, listed by alice at 500.00, with a hidden full-access key.
    let mut c = cheat_world();
    c.by_1y(ALICE)
        .list_for_sale(U128(500 * USD), CODE_HASH.into(), None);
    let cheat = snapshot(
        &mut c,
        json!({ (alice_key.clone()): ALICE }),
        Some(key_str(HIDDEN_KEY)),
    );

    // Recorded buy: summarizer buys translator with what its scan saw.
    let version = t.m().listing_version;
    let hash = t.c.known_keys_hash();
    let summ_key = key_str(SUMMARIZER_RUNNER);
    let unused = t.ft(
        SUMMARIZER,
        1_000 * USD,
        &Sim::buy_msg(version, &hash, SUMMARIZER_RUNNER),
        300,
    );
    assert_eq!(unused, None, "buy must be in flight");
    let settling = events("settling");
    let key_batch = key_actions(AGENT);
    let sale_id = t.m().pending.as_ref().expect("settling").sale_id;
    let refund = t.callback(true).on_handover(sale_id).0;
    let sale_event = events("sale");
    t.callback(true)
        .on_transfer_out(OutKind::SellerPayout, acc(ALICE), U128(700 * USD), None);
    let probes = json!({
        "withdraw": refusal(|| { t.by_1y(ALICE).withdraw(U128(USD), None); }),
        "set_limits": refusal(|| { t.by_1y(ALICE).set_limits(demo_limits()); }),
        "list_for_sale": refusal(|| { t.by_1y(ALICE).list_for_sale(U128(USD), CODE_HASH.into(), None); }),
        "nft_transfer": refusal(|| {
            t.by_1y(ALICE).nft_transfer(acc(ALICE), agent_exit::DEED_ID.into(), None, None);
        }),
    });
    let owners = json!({ (alice_key.clone()): ALICE, (summ_key.clone()): SUMMARIZER });
    let after_buy = snapshot(&mut t, owners.clone(), None);

    // Recorded resale listing (S15): summarizer lists translator at 1,200.00.
    let resale_card = t
        .by_1y(SUMMARIZER)
        .list_for_sale(U128(1_200 * USD), CODE_HASH.into(), None);
    let after_relist = snapshot(&mut t, owners, None);

    let world = json!({
        "schema": "mcp-world.v1",
        "_note": "RECORDED from the real contract code in near-sdk's mocked runtime by `cargo run -q -p verify --bin gen-mcp-world`. All money is xUSD-FAKE (no value). Not testnet, not mainnet.",
        "network": "offline",
        "token": TOKEN,
        "decimals": 6,
        "expected_code_hash": FIXTURE_CODE_HASH,
        "agents": {
            (AGENT): translator,
            (CHEAT): cheat,
        },
        "recorded_buys": [{
            "agent": AGENT,
            "buyer": SUMMARIZER,
            "buyer_runner_pk": summ_key,
            "amount": (1_000 * USD).to_string(),
            "listing_version": version,
            "known_keys_hash": hash,
            "settling": settling,
            "key_batch": key_batch.iter().map(|(a, k)| json!({"action": a, "key": k})).collect::<Vec<_>>(),
            "overpay_refund": refund.to_string(),
            "sale": sale_event,
            "old_owner_probes": {"account": ALICE, "results": probes},
            "after": after_buy,
        }],
        "recorded_listings": [{
            "agent": AGENT,
            "seller": SUMMARIZER,
            "price": (1_200 * USD).to_string(),
            "listing": resale_card,
            "after": after_relist,
        }],
    });
    serde_json::to_string_pretty(&world).expect("json") + "\n"
}

fn main() {
    std::panic::set_hook(Box::new(|_| {}));
    let path = concat!(env!("CARGO_MANIFEST_DIR"), "/../../fixtures/mcp/world.json");
    let text = build();
    let _ = std::panic::take_hook();
    if std::env::args().any(|a| a == "--check") {
        let have = std::fs::read_to_string(path).unwrap_or_default();
        if have != text {
            eprintln!("{path} differs from what the generator makes");
            std::process::exit(1);
        }
        eprintln!("{path} is reproducible");
    } else {
        std::fs::create_dir_all(std::path::Path::new(path).parent().unwrap()).expect("mkdir");
        std::fs::write(path, text).expect("write");
        eprintln!("wrote {path}");
    }
}
