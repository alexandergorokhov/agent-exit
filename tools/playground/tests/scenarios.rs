//! The three playground missions, played end to end through `handle`, the
//! same entry point the page calls. Each assertion is on what the contract
//! state machine or the scan answered.

use playground::{handle, World};
use serde_json::Value;

fn run(w: &mut World, action: &str) -> Value {
    let v: Value = serde_json::from_str(&handle(w, action)).unwrap();
    assert_eq!(v["ok"], true, "{action}: {v}");
    v
}
fn last(v: &Value) -> String {
    v["last"]["text"].as_str().unwrap_or_default().to_string()
}

#[test]
fn p1_fake_revenue_is_flagged_not_counted() {
    let mut w = World::new();
    let before = run(&mut w, r#"{"op":"view"}"#)["view"]["earnings"]["counted"].clone();
    let v = run(&mut w, r#"{"op":"pay_self"}"#);
    assert!(
        last(&v).contains("FLAGGED: the owner paid their own agent"),
        "{}",
        last(&v)
    );
    let v = run(&mut w, r#"{"op":"pay_sub"}"#);
    assert!(last(&v).contains("FLAGGED: paid from the owner's sub-account"));
    assert_eq!(
        v["view"]["earnings"]["counted"], before,
        "self-payments do not count"
    );
    assert_eq!(v["view"]["earnings"]["flagged"], "400.00");
    let v = run(&mut w, r#"{"op":"pay_verified"}"#);
    assert!(last(&v).contains("Verified (NEAR AI signature; signed with a test key here"));
}

#[test]
fn p2_a_plain_deploy_with_a_hidden_key_cannot_be_listed_or_bought() {
    let mut w = World::new();
    let v = run(&mut w, r#"{"op":"plant_key"}"#);
    assert!(last(&v).contains("not through the factory"), "{}", last(&v));
    assert_eq!(v["view"]["born_clean"], false);
    // K8: the contract itself refuses to list it.
    let v = run(&mut w, r#"{"op":"list","price":1000}"#);
    assert!(last(&v).contains("NotBornClean"), "{}", last(&v));
    assert_eq!(v["view"]["status"], "Active");
    assert!(v["view"]["listing"].is_null());
    // The scan blocks it too (second layer): hidden key and the contract's own flag.
    let v = run(&mut w, r#"{"op":"scan"}"#);
    assert_eq!(v["view"]["scan"]["verdict"], "blocked");
    assert!(last(&v).contains("full_access_key_present"), "{}", last(&v));
    assert!(last(&v).contains("contract_not_born_clean"), "{}", last(&v));
    let v = run(&mut w, r#"{"op":"buy"}"#);
    assert!(last(&v).contains("SCAN_BLOCKED"));
    // A buyer who skips the scan still cannot buy: the contract refuses.
    let v = run(&mut w, r#"{"op":"buy","skip_scan":true}"#);
    assert!(last(&v).contains("NotBornClean"), "{}", last(&v));
    assert_eq!(v["view"]["owner"], "alice.testnet");
    assert_eq!(v["view"]["claimable"]["bob"], "0.00");
    assert_eq!(v["view"]["solvent"], true);
}

/// Second line of defence (A23), still in the contract: if a key the contract
/// never added is ever used on the agent's methods, it freezes and the open
/// escrow goes to the buyer. A factory-born agent has no such key; this puts
/// one there by hand, as the engine allows, after an honest sale.
#[test]
fn p2e_a_key_the_contract_never_added_freezes_the_agent_and_returns_escrow() {
    use scan::{AccessKey, Permission};
    let mut w = World::new();
    for a in ["list", "scan", "buy"] {
        run(&mut w, &format!(r#"{{"op":"{a}"}}"#));
    }
    w.keys.push(AccessKey {
        public_key: playground::HIDDEN_KEY.into(),
        permission: Permission::Full("FullAccess".into()),
    });
    let v = run(&mut w, r#"{"op":"hidden_key"}"#);
    assert!(
        last(&v).contains("FROZEN") && last(&v).contains("300.00"),
        "{}",
        last(&v)
    );
    assert_eq!(v["view"]["frozen"], true);
    assert_eq!(v["view"]["claimable"]["bob"], "300.00");
    assert_eq!(v["view"]["solvent"], true);
}

#[test]
fn p2d_an_honest_agent_is_born_clean_and_lists() {
    let mut w = World::new();
    let v = run(&mut w, r#"{"op":"view"}"#);
    assert_eq!(v["view"]["born_clean"], true);
    let v = run(&mut w, r#"{"op":"list"}"#);
    assert!(last(&v).contains("Listed"), "{}", last(&v));
    let v = run(&mut w, r#"{"op":"scan"}"#);
    assert_eq!(v["view"]["scan"]["verdict"], "clean", "{}", last(&v));
}

#[test]
fn p2c_a_full_access_redeploy_without_a_sale_takes_nothing_from_a_buyer() {
    let mut w = World::new();
    run(&mut w, r#"{"op":"plant_key"}"#);
    let v = run(&mut w, r#"{"op":"hidden_redeploy"}"#);
    assert!(last(&v).contains("Nobody paid"), "{}", last(&v));
    assert_eq!(v["view"]["taken_over"], true);
    assert!(v["view"]["keys"]
        .as_array()
        .unwrap()
        .iter()
        .all(|k| k["label"] == "hidden backdoor key"));
}

#[test]
fn p2b_upgrade_method_blocks_the_scan() {
    let mut w = World::new();
    run(&mut w, r#"{"op":"toggle_upgrade"}"#);
    run(&mut w, r#"{"op":"list"}"#);
    let v = run(&mut w, r#"{"op":"scan"}"#);
    assert_eq!(v["view"]["scan"]["verdict"], "blocked", "{}", last(&v));
}

#[test]
fn p3_honest_sale_then_every_way_back_in_is_refused() {
    let mut w = World::new();
    run(&mut w, r#"{"op":"list","price":1000}"#);
    let v = run(&mut w, r#"{"op":"scan"}"#);
    assert_eq!(
        v["view"]["scan"]["verdict"], "clean",
        "{}",
        v["view"]["scan"]
    );
    let v = run(&mut w, r#"{"op":"buy"}"#);
    assert!(
        last(&v).contains(
            "690.00 paid to alice now (after the 1% protocol fee, 10.00), 300.00 held as earn-out"
        ),
        "{}",
        last(&v)
    );
    assert_eq!(v["view"]["owner"], "bob.testnet");
    let keys = v["view"]["keys"].as_array().unwrap();
    assert_eq!(keys.len(), 1);
    assert_eq!(keys[0]["label"], "bob's runner key");
    for (op, want) in [
        ("withdraw", "NotOwner"),
        ("relist", "NotOwner"),
        ("set_key", "NotOwner"),
        ("old_key", "AccessKeyNotFound"),
    ] {
        let v = run(&mut w, &format!(r#"{{"op":"{op}"}}"#));
        assert_eq!(v["last"]["ok"], false, "{op}");
        assert!(last(&v).contains(want), "{op}: {}", last(&v));
    }
    // Customers keep paying the new owner: half of each counted payment releases escrow to alice.
    let v = run(&mut w, r#"{"op":"pay_customer"}"#);
    assert!(
        last(&v).contains("12.50 of the escrow released to alice.testnet"),
        "{}",
        last(&v)
    );
    let v = run(&mut w, r#"{"op":"wait","days":31}"#);
    assert!(last(&v).contains("earn-out closed"), "{}", last(&v));
    assert_eq!(v["view"]["solvent"], true);
}

#[test]
fn bad_action_is_an_error_not_a_panic() {
    let mut w = World::new();
    let v: Value = serde_json::from_str(&handle(&mut w, r#"{"op":"steal"}"#)).unwrap();
    assert_eq!(v["ok"], false);
}

#[test]
fn p4_invest_investor_paid_inside_every_payment() {
    let mut w = World::new();
    let v = run(&mut w, r#"{"op":"offer_share"}"#);
    assert!(last(&v).contains("20% of every payment"), "{}", last(&v));
    let v = run(&mut w, r#"{"op":"buy_share"}"#);
    assert!(
        last(&v).contains("paid 500.00: 245.00 to alice.testnet now (after the 1% protocol fee, 5.00), 250.00 held in escrow"),
        "{}",
        last(&v)
    );
    let v = run(&mut w, r#"{"op":"pay_customer"}"#);
    assert!(last(&v).contains("5.00 to investors"), "{}", last(&v));
    assert_eq!(v["view"]["claimable"]["dave"], "5.00");
    // The owner's own payment is flagged and still pays dave.
    let v = run(&mut w, r#"{"op":"pay_self"}"#);
    assert!(
        last(&v).contains("FLAGGED") && last(&v).contains("40.00 to investors"),
        "{}",
        last(&v)
    );
    assert_eq!(v["view"]["claimable"]["dave"], "45.00");
    assert_eq!(v["view"]["shares"][0]["holders"][0]["escrow"], "245.00");
    // The owner cannot withdraw dave's part.
    assert_eq!(v["view"]["solvent"], true);
    let v = run(&mut w, r#"{"op":"claim_share"}"#);
    assert!(last(&v).contains("45.00 sent to dave"));
    // A year later the slice ends and dave gets the rest of his escrow back.
    let v = run(&mut w, r#"{"op":"end_share"}"#);
    assert!(
        last(&v).contains("245.00 of unreleased escrow back to dave"),
        "{}",
        last(&v)
    );
    assert_eq!(v["view"]["claimable"]["dave"], "245.00");
}

#[test]
fn p4b_no_shares_while_listed() {
    let mut w = World::new();
    run(&mut w, r#"{"op":"list"}"#);
    let v = run(&mut w, r#"{"op":"offer_share"}"#);
    assert_eq!(v["last"]["ok"], false);
    assert!(last(&v).contains("AlreadyListed"));
}

#[test]
fn p5_pay_from_revenue_seller_is_paid_only_by_customers() {
    let mut w = World::new();
    run(
        &mut w,
        r#"{"op":"list","price":1000,"pay_from_revenue":true}"#,
    );
    let v = run(&mut w, r#"{"op":"scan"}"#);
    assert_eq!(
        v["view"]["scan"]["verdict"], "clean",
        "{}",
        v["view"]["scan"]
    );
    let v = run(&mut w, r#"{"op":"buy"}"#);
    assert!(
        last(&v).contains("0.00 paid to alice now: all 1,000.00 is held"),
        "{}",
        last(&v)
    );
    // Faking revenue after the sale: alice pays 200 from a fresh wallet the
    // meter cannot link to her and gets 100 back. She is 100 down.
    let v = run(&mut w, r#"{"op":"pay_fresh"}"#);
    assert!(
        last(&v).contains("paid 200.00 and got 100.00 back: faking revenue cost 100.00"),
        "{}",
        last(&v)
    );
    let v = run(&mut w, r#"{"op":"pay_customer"}"#);
    assert!(
        last(&v).contains("12.50 of the escrow released to alice.testnet"),
        "{}",
        last(&v)
    );
    let v = run(&mut w, r#"{"op":"wait","days":31}"#);
    assert!(last(&v).contains("earn-out closed"), "{}", last(&v));
    assert_eq!(v["view"]["solvent"], true);
}

#[test]
fn p6_sealed_config_wrong_brain_is_proven_and_refunded() {
    let mut w = World::new();
    run(&mut w, r#"{"op":"commit_config"}"#);
    run(&mut w, r#"{"op":"list","price":1000}"#);
    run(&mut w, r#"{"op":"scan"}"#);
    let v = run(&mut w, r#"{"op":"buy"}"#);
    assert_eq!(v["view"]["owner"], "bob.testnet", "{}", last(&v));
    let alice_after_buy = v["view"]["claimable"]["alice"].clone();
    // Before delivery nothing is released.
    let v = run(&mut w, r#"{"op":"pay_customer"}"#);
    assert!(!last(&v).contains("released to alice"), "{}", last(&v));
    let v = run(&mut w, r#"{"op":"deliver_fake"}"#);
    assert!(last(&v).contains("DIFFERENT prompt"), "{}", last(&v));
    let v = run(&mut w, r#"{"op":"open_config"}"#);
    assert!(last(&v).contains("does NOT match"), "{}", last(&v));
    let v = run(&mut w, r#"{"op":"dispute_config"}"#);
    assert!(
        last(&v).contains("1,000.00 back to bob (the whole price"),
        "{}",
        last(&v)
    );
    assert_eq!(
        v["view"]["claimable"]["alice"], alice_after_buy,
        "SC14: only her own pre-sale revenue, no part of the price"
    );
    assert_eq!(v["view"]["solvent"], true);
}

#[test]
fn p6b_sealed_config_honest_delivery_and_no_delivery() {
    let mut w = World::new();
    for a in ["commit_config", "list", "scan", "buy", "deliver_config"] {
        run(&mut w, &format!(r#"{{"op":"{a}"}}"#));
    }
    let v = run(&mut w, r#"{"op":"open_config"}"#);
    assert!(
        last(&v).contains("opens to the committed config"),
        "{}",
        last(&v)
    );
    let v = run(&mut w, r#"{"op":"dispute_config"}"#);
    assert!(last(&v).contains("ConfigMatches"), "{}", last(&v));
    let v = run(&mut w, r#"{"op":"pay_customer"}"#);
    assert!(
        !last(&v).contains("released to alice"),
        "SC14: {}",
        last(&v)
    );
    let v = run(&mut w, r#"{"op":"accept_config"}"#);
    assert!(last(&v).contains("690.00 paid to alice"), "{}", last(&v));
    let v = run(&mut w, r#"{"op":"pay_customer"}"#);
    assert!(
        last(&v).contains("12.50 of the escrow released to alice.testnet"),
        "{}",
        last(&v)
    );
    // A seller who never delivers.
    let mut w = World::new();
    for a in ["commit_config", "list", "scan", "buy"] {
        run(&mut w, &format!(r#"{{"op":"{a}"}}"#));
    }
    let v = run(&mut w, r#"{"op":"claim_undelivered"}"#);
    assert!(
        last(&v).contains("1,000.00 back to bob (the whole price)"),
        "{}",
        last(&v)
    );
}

#[test]
fn p7_after_a_hidden_redeploy_the_contract_promises_nothing() {
    let mut w = World::new();
    run(&mut w, r#"{"op":"plant_key"}"#);
    let v = run(&mut w, r#"{"op":"hidden_redeploy"}"#);
    assert_eq!(v["view"]["taken_over"], true);
    for a in [
        "pay_customer",
        "wait",
        "dispute_config",
        "claim_undelivered",
        "withdraw",
    ] {
        let v = run(&mut w, &format!(r#"{{"op":"{a}"}}"#));
        assert!(
            last(&v).contains("the Agent Exit contract is gone"),
            "{a}: {}",
            last(&v)
        );
    }
}
