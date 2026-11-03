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
fn p2_hidden_key_blocks_the_scan_and_freezes_if_skipped() {
    let mut w = World::new();
    run(&mut w, r#"{"op":"plant_key"}"#);
    run(&mut w, r#"{"op":"list","price":1000}"#);
    let v = run(&mut w, r#"{"op":"scan"}"#);
    assert_eq!(v["view"]["scan"]["verdict"], "blocked");
    assert!(last(&v).contains("full_access_key_present"), "{}", last(&v));
    let v = run(&mut w, r#"{"op":"buy"}"#);
    assert!(last(&v).contains("SCAN_BLOCKED"));
    assert_eq!(v["view"]["owner"], "alice.testnet");
    // A buyer who skips the scan gets the agent, but the hidden key is still there.
    let v = run(&mut w, r#"{"op":"buy","skip_scan":true}"#);
    assert_eq!(v["view"]["owner"], "bob.testnet");
    assert!(last(&v).contains("hidden key is still on the account"));
    assert_eq!(v["view"]["escrow_left"], "300.00");
    // alice uses it: the contract proves the backdoor, freezes, escrow goes to bob.
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
fn p2c_skipping_the_scan_lets_a_hidden_key_take_the_agent_back() {
    let mut w = World::new();
    run(&mut w, r#"{"op":"plant_key"}"#);
    run(&mut w, r#"{"op":"list"}"#);
    run(&mut w, r#"{"op":"buy","skip_scan":true}"#);
    let v = run(&mut w, r#"{"op":"hidden_redeploy"}"#);
    assert!(
        last(&v).contains("Only the scan protects the buyer"),
        "{}",
        last(&v)
    );
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
        last(&v).contains("700.00 paid to alice now, 300.00 held as earn-out"),
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
