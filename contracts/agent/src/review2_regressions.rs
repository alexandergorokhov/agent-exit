// Promises are dropped on purpose (the mocked runtime still schedules them).
#![allow(unused_must_use)]

//! Regression tests for security review 2. Each test is a review scratch test
//! (which asserted the defect) with its asserts inverted: it passes only when
//! the defect is gone. Scenario ids A31 and up.

use crate::sim::*;
use crate::*;

fn panic_text<F: FnOnce()>(f: F) -> String {
    let raw = match std::panic::catch_unwind(std::panic::AssertUnwindSafe(f)) {
        Ok(()) => return String::new(),
        Err(e) => e
            .downcast_ref::<String>()
            .cloned()
            .or_else(|| e.downcast_ref::<&str>().map(|s| s.to_string()))
            .unwrap_or_default(),
    };
    match raw.split_once("panic_msg: \"") {
        Some((_, rest)) => rest
            .rsplit_once("\" }")
            .map(|x| x.0)
            .unwrap_or(rest)
            .to_string(),
        None => raw,
    }
}

/// A31 (review 2 H1, R1): after the sale the buyer withdraws 1 base unit to
/// every real customer. Their later payments are still flagged `round_trip`
/// in the meter (the next buyer sees them), but they keep releasing the
/// seller's earn-out: only round trips from before the sale count against it.
#[test]
fn a31_buyer_round_trips_do_not_starve_earnout() {
    let mut s = sold_translator();
    s.advance(DAY);
    s.pay(PAYERS[0], 12 * USD);
    let released_before = s.m().sales[0].released;
    assert!(released_before > 0, "honest payment released earn-out");
    for p in PAYERS {
        s.by_1y(BOB).withdraw(U128(1), Some(acc(p)));
    }
    for p in PAYERS {
        s.pay(p, 100 * USD);
    }
    // 50% of each 100.00 payment is released (share 50%), up to the
    // 300.00 escrow.
    assert_eq!(
        s.m().sales[0].released,
        300 * USD,
        "customer payments released the whole escrow"
    );
    assert!(s.by(BOB).get_earnings().flagged_round_trip >= 600 * USD);
    s.advance(31 * DAY);
    s.by(MALLORY).finalize_earnout(1);
    let sale = &s.m().sales[0];
    assert_eq!(sale.released, 300 * USD, "the seller got the whole escrow");
    assert_eq!(sale.returned, 0);
}

/// A32 (review 2 H2): the contract refuses a listing below the published
/// minimum terms (30% escrow, seller floor at most 50% of it, 30 days), so
/// the earn-out protection does not depend on the seller. The minimum and
/// its `prod`/`demo` label are in `get_status` and every listing.
#[test]
fn a32_contract_refuses_terms_below_minimum() {
    let mut s = translator_world();
    let base = agent_core::Terms::default();
    for t in [
        agent_core::Terms {
            earnout_bps: 0,
            ..base
        },
        agent_core::Terms {
            earnout_bps: 2_999,
            ..base
        },
        agent_core::Terms {
            floor_bps: 10_000,
            ..base
        },
        agent_core::Terms {
            window_secs: 60,
            ..base
        },
    ] {
        let msg = panic_text(|| {
            s.by_1y(ALICE)
                .list_for_sale(U128(1_000 * USD), CODE_HASH.into(), Some(t));
        });
        assert_eq!(msg, "BadTerms", "{t:?}");
    }
    assert_eq!(s.m().status, agent_core::Status::Active);
    let st = s.by(MALLORY).get_status();
    assert_eq!(st["terms_config"], "prod");
    assert_eq!(st["terms_min"]["min_earnout_bps"], 3_000);
    assert_eq!(st["terms_min"]["min_window_secs"], 30 * 86_400);
    let card = s
        .by_1y(ALICE)
        .list_for_sale(U128(1_000 * USD), CODE_HASH.into(), Some(base));
    assert_eq!(card["terms_config"], "prod");
}

/// A33 (review 2 M1): `on_handover` fails after the key batch ran (out of
/// gas, say). The token refunds the buyer in full; before the fix the agent
/// stayed `Settling` for good. Now anyone can undo the buy an hour later:
/// the seller keeps the deed, the listing is withdrawn, every key the buy
/// touched is deleted in its own receipt, and the seller can run again.
#[test]
fn a33_failed_handover_callback_is_recoverable() {
    let mut s = listed_translator();
    assert_eq!(s.buy(BOB, 1_000 * USD, BOB_RUNNER), None);
    // FAULT INJECTION: on_handover never runs.
    for (who, f) in [
        (
            "withdraw",
            Box::new(|s: &mut Sim| {
                s.by_1y(ALICE).withdraw(U128(USD), None);
            }) as Box<dyn Fn(&mut Sim)>,
        ),
        (
            "cancel",
            Box::new(|s: &mut Sim| {
                s.by_1y(ALICE).cancel_listing();
            }),
        ),
    ] {
        assert_eq!(panic_text(|| f(&mut s)), "SaleInProgress", "{who}");
    }
    s.advance(30 * 60 * agent_core::NANOS_PER_SEC);
    assert_eq!(
        panic_text(|| {
            s.by(MALLORY).recover_sale();
        }),
        "RecoverTooEarly"
    );
    s.advance(31 * 60 * agent_core::NANOS_PER_SEC);
    let r = s.by(MALLORY).recover_sale();
    assert_eq!(r["buyer"], BOB);
    assert_eq!(s.m().owner, ALICE);
    assert_eq!(s.m().status, agent_core::Status::Active);
    assert!(s.m().pending.is_none() && s.m().listing.is_none());
    assert!(s.m().sales.is_empty());
    assert!(s.m().solvent());
    assert_eq!(s.m().held, s.m().owner_free + s.m().treasury);
    // Each key in its own receipt, so a key that never got added (or is
    // already gone) cannot fail the other deletions.
    assert_eq!(key_receipts(AGENT), 2);
    assert!(s.m().retiring.contains(&key_str(BOB_RUNNER)));
    // Still pending until the deletions are confirmed: no freeze.
    assert_eq!(
        panic_text(|| {
            s.runner(key(BOB_RUNNER)).heartbeat();
        }),
        "NotRunner"
    );
    s.callback(false)
        .on_keys_deleted(vec![key_str(ALICE_RUNNER), key_str(BOB_RUNNER)]);
    assert!(s.m().retiring.is_empty() && s.m().incoming.is_empty());
    // The seller runs, withdraws and lists again.
    s.by_1y(ALICE).withdraw(U128(USD), None);
    s.by_1y(ALICE).set_runner_key(key(9));
    s.callback(true).on_runner_key(vec![], vec![], key_str(9));
    assert_eq!(s.runner(key(9)).heartbeat()["ok"], true);
    s.by_1y(ALICE)
        .list_for_sale(U128(1_000 * USD), CODE_HASH.into(), None);
    assert_eq!(s.m().status, agent_core::Status::Listed);
}

/// A33: `on_runner_key` fails after the rotation batch ran. The new key
/// stays `incoming` and every key or listing path says `KeysPending`. The
/// owner recovers an hour later; the agent can rotate and list again.
#[test]
fn a33_failed_rotation_callback_is_recoverable() {
    let mut s = sold_translator();
    s.by_1y(BOB).set_runner_key(key(CAROL_RUNNER));
    // FAULT INJECTION: on_runner_key never runs.
    assert_eq!(
        panic_text(|| {
            s.by_1y(BOB)
                .list_for_sale(U128(1_000 * USD), CODE_HASH.into(), None);
        }),
        "KeysPending"
    );
    assert_eq!(
        panic_text(|| {
            s.by_1y(MALLORY).recover_keys();
        }),
        "NotOwner"
    );
    assert_eq!(
        panic_text(|| {
            s.by_1y(BOB).recover_keys();
        }),
        "RecoverTooEarly"
    );
    s.advance(agent_core::RECOVER_AFTER_NS);
    let r = s.by_1y(BOB).recover_keys();
    let keys: Vec<String> = serde_json::from_value(r["delete_keys"].clone()).unwrap();
    assert_eq!(keys.len(), 2, "old and new key");
    assert_eq!(key_receipts(AGENT), 2);
    s.callback(true).on_keys_deleted(keys);
    assert!(s.m().incoming.is_empty() && s.m().retiring.is_empty());
    assert!(s.m().paused && !s.m().frozen);
    s.by_1y(BOB).set_runner_key(key(9));
    s.callback(true).on_runner_key(vec![], vec![], key_str(9));
    assert_eq!(s.runner(key(9)).heartbeat()["ok"], true);
}

/// A33: `on_keys_deleted` fails after a deed transfer. The old keys stay
/// `retiring`; the new owner recovers them, and a later rotation never puts
/// a maybe-gone key into its all-or-nothing batch.
#[test]
fn a33_failed_deletion_callback_is_recoverable() {
    let mut s = translator_world();
    s.by_1y(ALICE)
        .nft_transfer(acc(CAROL), DEED_ID.into(), None, None);
    // FAULT INJECTION: on_keys_deleted never runs.
    assert_eq!(
        panic_text(|| {
            s.by_1y(CAROL).set_runner_key(key(CAROL_RUNNER));
        }),
        "KeysPending"
    );
    s.advance(agent_core::RECOVER_AFTER_NS);
    s.by_1y(CAROL).recover_keys();
    s.callback(false)
        .on_keys_deleted(vec![key_str(ALICE_RUNNER)]);
    assert!(s.m().retiring.is_empty());
    s.by_1y(CAROL).set_runner_key(key(CAROL_RUNNER));
    assert_eq!(
        key_actions(AGENT),
        vec![("add_fc".to_string(), key_str(CAROL_RUNNER))],
        "the rotation deletes no unconfirmed key"
    );
}

/// A33 (review 2 R4, inverted): every callback gets gas for reading and
/// writing the whole state on top of its own work, so outsiders' payments
/// (1,000 payers here) cannot push a callback out of gas.
#[test]
fn a33_callback_gas_grows_with_state() {
    let mut s = sold_translator();
    for i in 0..1000 {
        let p = format!("payer-with-a-longish-name-{i:04}.testnet");
        s.pay(&p, USD / 10);
    }
    let bytes = near_sdk::borsh::to_vec(&s.c).unwrap().len() as u64;
    assert!(bytes > 100_000, "{bytes}");
    let mut c = s.call(BOB);
    c.deposit = near_sdk::NearToken::from_yoctonear(1);
    c.storage = bytes;
    s.env(&c);
    s.c.set_runner_key(key(CAROL_RUNNER));
    let cb = self_calls(AGENT);
    let (_, gas) = cb
        .iter()
        .find(|(m, _)| m == "on_runner_key")
        .expect("callback");
    let state_io = near_sdk::Gas::from_gas(bytes * GAS_PER_STATE_BYTE);
    assert!(state_io.as_tgas() >= 7, "{state_io}");
    assert_eq!(*gas, GAS_ON_KEYS.saturating_add(state_io));
}

/// A34 (review 2 M2, R2 inverted): a hidden key proven while a buy is
/// settling. Before the fix the sale still completed: the seller got 70% and
/// the buyer a permanently frozen agent. Now the buy is undone: the buyer is
/// refunded in full, the seller pays out nothing and keeps the frozen agent,
/// and the buyer's key (added by the batch) is deleted.
#[test]
fn a34_freeze_during_settling_refunds_the_buyer() {
    let mut s = listed_translator();
    assert_eq!(s.buy(BOB, 1_000 * USD, BOB_RUNNER), None);
    let v = s.runner(key(HIDDEN_KEY)).heartbeat();
    assert_eq!(v["frozen"], true);
    let unused = s.callback(true).on_handover(1).0;
    assert_eq!(unused, 1_000 * USD, "the buyer gets everything back");
    assert_eq!(s.m().owner, ALICE);
    assert!(s.m().frozen);
    assert!(s.m().sales.is_empty(), "no sale happened");
    assert!(
        ft_transfers().iter().all(|(to, _)| to != ALICE),
        "no payout to the seller"
    );
    assert_eq!(
        key_actions(AGENT),
        vec![("delete".to_string(), key_str(BOB_RUNNER))]
    );
    assert_eq!(s.m().status, agent_core::Status::Active);
    assert!(s.m().solvent());
    assert_eq!(s.m().held, s.m().owner_free + s.m().treasury);
    // Nobody can list a frozen agent again.
    assert_eq!(
        panic_text(|| {
            s.by_1y(ALICE)
                .list_for_sale(U128(1_000 * USD), CODE_HASH.into(), None);
        }),
        "Frozen"
    );
}

/// A34: on a frozen agent the runner can never spend the treasury again, so
/// the owner may withdraw it (revenue first, then treasury).
#[test]
fn a34_owner_withdraws_treasury_of_frozen_agent() {
    let mut s = business_world();
    let treasury = s.m().treasury;
    let free = s.m().owner_free;
    assert!(treasury > 0);
    assert_eq!(
        panic_text(|| {
            s.by_1y(BOB).withdraw(U128(free + treasury), None);
        }),
        "InsufficientFunds",
        "not while the runner can still spend it"
    );
    s.runner(key(HIDDEN_KEY)).heartbeat();
    assert!(s.m().frozen);
    s.by_1y(BOB).withdraw(U128(free + treasury), None);
    assert_eq!(s.m().treasury, 0);
    assert_eq!(s.m().owner_free, 0);
    assert!(s.m().solvent());
}

/// A35 (review 2 M3, R3 inverted): the receiver of `nft_transfer_call`
/// rotates the runner key inside `nft_on_transfer` and then asks for the
/// deed back. The return is refused (`KeysPending`), so resolve reports that
/// the receiver kept the deed (`true`) and emits no NEP-171 transfer back.
#[test]
fn a35_resolve_reports_a_refused_return_truthfully() {
    let mut s = sold_translator();
    s.by_1y(BOB)
        .nft_transfer_call(acc(CAROL), DEED_ID.into(), None, None, String::new());
    s.callback(true).on_keys_deleted(vec![key_str(BOB_RUNNER)]);
    // The receiver (carol) acts as owner inside nft_on_transfer.
    s.by_1y(CAROL).set_runner_key(key(CAROL_RUNNER));
    // nft_on_transfer returned true: "give it back".
    near_sdk::testing_env!(
        near_sdk::test_utils::VMContextBuilder::new()
            .current_account_id(acc(AGENT))
            .predecessor_account_id(acc(AGENT))
            .block_timestamp(s.now)
            .build(),
        near_sdk::test_vm_config(),
        near_sdk::RuntimeFeesConfig::test(),
        Default::default(),
        vec![near_sdk::PromiseResult::Successful(b"true".to_vec())]
    );
    let kept =
        s.c.nft_resolve_transfer(acc(BOB), acc(CAROL), DEED_ID.into(), None);
    assert!(kept, "resolve says the receiver kept the deed");
    assert_eq!(s.m().owner, CAROL);
    assert!(!logs().iter().any(|l| l.contains("agent_exit:returned")));
    assert_eq!(events("deed_return_refused")[0]["reason"], "KeysPending");
}
