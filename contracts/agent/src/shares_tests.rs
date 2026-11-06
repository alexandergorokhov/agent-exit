// Promises returned by contract methods are dropped on purpose (see tests.rs).
#![allow(unused_must_use)]

//! Shares of future earnings through the contract (PLAN-shares.md): the
//! `buy_share` message, the events and the views, in the mocked runtime.

use crate::sim::*;
use crate::*;

const DAVE: &str = "dave.testnet";
const YEAR: u64 = 365 * DAY;

fn share_msg(o: &Value, part: u16) -> String {
    json!({"kind": "buy_share", "offer_id": o["offer_id"], "bps": o["bps"],
           "price": o["price"], "end": o["end"], "part": part})
    .to_string()
}

/// translator (fixed world) offers 20% for a year at 500; dave buys it all.
fn dave_holds() -> (Sim, Value) {
    let mut s = translator_world();
    let end = U64(s.now + YEAR);
    let o = s.by_1y(ALICE).offer_share(2_000, U128(500 * USD), end);
    assert_eq!(events("share_offered").len(), 1);
    assert_eq!(
        s.ft(DAVE, 600 * USD, &share_msg(&o, 2_000), 100),
        Some(100 * USD)
    );
    assert_eq!(events("share_bought")[0]["cost"], "500000000");
    // 1% protocol fee (5.00) out of alice's upfront 250.00, claimable by the fee account.
    assert_eq!(events("share_bought")[0]["protocol_fee"], "5000000");
    assert_eq!(s.c.get_claimable(acc(crate::FEE_ACCOUNT)).0, 5 * USD);
    (s, o)
}

#[test]
fn sh_buy_pay_claim_through_the_contract() {
    let (mut s, _) = dave_holds();
    s.now += DAY;
    s.pay(PAYERS[0], 100 * USD);
    assert_eq!(events("split")[0]["to_investors"], "20000000");
    assert_eq!(events("share_paid")[0]["amount"], "20000000");
    let h = s.c.get_holdings(acc(DAVE));
    assert_eq!(h["claimable"], "20000000");
    assert_eq!(h["holdings"][0]["earned"], "20000000");
    assert_eq!(h["holdings"][0]["escrow"], "230000000");
    let st = s.c.get_status();
    assert_eq!(st["shares_bps"], 2_000);
    assert_eq!(st["share_escrow"], "230000000");
    assert_eq!(st["solvent"], true);
    s.by(DAVE).claim();
    assert_eq!(s.c.get_claimable(acc(DAVE)).0, 0);
}

#[test]
fn t6_changed_terms_refused_full_refund() {
    let mut s = translator_world();
    let end = U64(s.now + YEAR);
    let o = s.by_1y(ALICE).offer_share(2_000, U128(500 * USD), end);
    let mut seen = o.clone();
    seen["price"] = json!("400000000");
    assert_eq!(
        s.ft(DAVE, 500 * USD, &share_msg(&seen, 2_000), 100),
        Some(500 * USD)
    );
    assert!(events("refused")
        .iter()
        .any(|e| e["reason"] == "ShareChanged"));
}

#[test]
fn sh7_listing_discloses_shares() {
    let (mut s, _) = dave_holds();
    let card = s
        .by_1y(ALICE)
        .list_for_sale(U128(1_000 * USD), CODE_HASH.into(), None);
    assert_eq!(card["shares_bps"], 2_000);
    assert_eq!(card["shares"][0]["holders"][0]["account"], DAVE);
}

#[test]
fn offer_needs_owner_and_one_yocto() {
    let mut s = translator_world();
    let end = U64(s.now + YEAR);
    let msg = |r: Box<dyn std::any::Any + Send>| {
        r.downcast_ref::<String>()
            .cloned()
            .or_else(|| r.downcast_ref::<&str>().map(|s| s.to_string()))
            .unwrap_or_default()
    };
    let e = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        s.by(ALICE).offer_share(2_000, U128(500 * USD), end);
    }))
    .unwrap_err();
    assert!(msg(e).contains("1 yoctoNEAR"), "needs one yocto");
    let e = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        s.by_1y(MALLORY).offer_share(2_000, U128(500 * USD), end);
    }))
    .unwrap_err();
    assert!(msg(e).contains("NotOwner"), "owner only");
}

#[test]
fn close_after_end_refunds_escrow() {
    let (mut s, o) = dave_holds();
    s.now += YEAR;
    let v = s.by(MALLORY).close_share(o["offer_id"].as_u64().unwrap());
    assert_eq!(v["refunds"][0]["amount"], "250000000");
    assert_eq!(s.c.get_claimable(acc(DAVE)).0, 250 * USD);
    assert!(s.c.get_shares().is_empty());
}
