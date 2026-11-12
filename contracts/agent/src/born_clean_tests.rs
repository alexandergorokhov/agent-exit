// Promises returned by contract methods are dropped on purpose (see tests.rs).
#![allow(unused_must_use)]

//! K8: the contract itself refuses to list or sell an agent that was not
//! created by the pinned factory (`NotBornClean`). The cheat is a plain
//! deploy (its deployer may hold a full-access key no contract can list);
//! the honest agents are children of the factory.

use crate::sim::*;
use crate::*;

#[test]
fn k8_factory_born_agent_is_clean_and_says_so() {
    let mut s = translator_world();
    assert!(s.m().born_clean);
    let st = s.by(BOB).get_status();
    assert_eq!(st["born_clean"], json!(true));
    assert_eq!(st["factory"], json!(crate::FACTORY));
    let deed = s.by(BOB).nft_token("deed".into()).expect("deed");
    let extra: Value = serde_json::from_str(&deed.metadata.unwrap().extra.unwrap()).unwrap();
    assert_eq!(extra["born_clean"], json!(true));
}

#[test]
fn k8_plain_deploy_is_not_clean_and_says_so() {
    let mut s = cheat_world();
    assert!(!s.m().born_clean);
    let st = s.by(BOB).get_status();
    assert_eq!(st["born_clean"], json!(false));
    let deed = s.by(BOB).nft_token("deed".into()).expect("deed");
    let extra: Value = serde_json::from_str(&deed.metadata.unwrap().extra.unwrap()).unwrap();
    assert_eq!(extra["born_clean"], json!(false));
}

#[test]
#[should_panic(expected = "NotBornClean")]
fn k8_list_for_sale_is_refused_on_a_plain_deploy() {
    let mut s = cheat_world();
    s.by_1y(ALICE)
        .list_for_sale(U128(1_000 * USD), CODE_HASH.into(), None);
}

#[test]
#[should_panic(expected = "NotBornClean")]
fn k8_pay_from_revenue_listing_is_refused_too() {
    let mut s = cheat_world();
    let terms = agent_core::Terms {
        earnout_bps: agent_core::MAX_EARNOUT_BPS,
        ..Default::default()
    };
    s.by_1y(ALICE)
        .list_for_sale(U128(1_000 * USD), CODE_HASH.into(), Some(terms));
}

#[test]
fn k8_nothing_is_listed_after_the_refusal_and_a_buy_finds_no_listing() {
    let mut s = cheat_world();
    let r = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        s.by_1y(ALICE)
            .list_for_sale(U128(1_000 * USD), CODE_HASH.into(), None);
    }));
    assert!(r.is_err());
    assert!(s.m().listing.is_none());
    // A buyer paying anyway gets everything back (NotListed or NotBornClean).
    let unused = s.buy(BOB, 1_000 * USD, BOB_RUNNER);
    assert_eq!(unused, Some(1_000 * USD));
    assert!(s.m().pending.is_none());
}

#[test]
fn k8_defence_in_depth_buy_refuses_even_with_a_forced_listing() {
    let mut s = listed_translator();
    s.c.m.born_clean = false;
    let unused = s.buy(BOB, 1_000 * USD, BOB_RUNNER);
    assert_eq!(unused, Some(1_000 * USD));
    assert!(s.m().pending.is_none());
    assert_eq!(s.m().status, agent_core::Status::Listed);
}

#[test]
fn k8_share_offers_stay_allowed_on_a_plain_deploy() {
    let mut s = cheat_world();
    let end = U64(s.now + 365 * DAY);
    let o = s.by_1y(ALICE).offer_share(2_000, U128(500 * USD), end);
    assert_eq!(o["bps"], json!(2_000));
}
