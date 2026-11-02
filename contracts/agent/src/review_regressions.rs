// Promises are dropped on purpose (the mocked runtime still schedules them).
#![allow(unused_must_use)]

//! Regression tests for security review 1. Each test is a proof-of-concept
//! attack from the review with its asserts inverted: it passes only when the
//! attack FAILS. Scenario ids A25-A30.

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

/// A25 (review C1a): the buyer's key is live after the key batch, before
/// `on_handover`. A heartbeat signed by it in that gap is NotRunner, not a
/// backdoor: no freeze, the escrow stays, the seller keeps the earn-out.
#[test]
fn a25_buyer_cannot_freeze_in_handover_gap() {
    let mut s = listed_translator();
    assert_eq!(s.buy(BOB, 1_000 * USD, BOB_RUNNER), None);
    let msg = panic_text(|| {
        s.runner(key(BOB_RUNNER)).heartbeat();
    });
    assert_eq!(msg, "NotRunner");
    assert!(!s.m().frozen);
    s.callback(true).on_handover(1);
    assert_eq!(s.c.get_claimable(acc(BOB)).0, 0, "buyer gets no escrow");
    assert_eq!(
        s.m().escrow_left(),
        300 * USD,
        "escrow intact for the earn-out"
    );
    assert!(!s.m().sales[0].cut);
    // The buyer's runner works once the handover is confirmed.
    assert_eq!(s.runner(key(BOB_RUNNER)).heartbeat()["ok"], true);
}

/// A25 (review C1b): during a rotation the old key is still on chain until
/// the batch runs. A call signed by it is NotRunner; the agent is not frozen
/// and the escrow is not refunded. The honest rotation completes and the
/// agent can still be listed.
#[test]
fn a25_rotation_gap_does_not_freeze() {
    let mut s = sold_translator();
    s.by_1y(BOB).set_runner_key(key(CAROL_RUNNER));
    for k in [BOB_RUNNER, CAROL_RUNNER] {
        let msg = panic_text(|| {
            s.runner(key(k)).heartbeat();
        });
        assert_eq!(msg, "NotRunner", "key {k}");
    }
    assert!(!s.m().frozen);
    assert_eq!(s.c.get_claimable(acc(BOB)).0, 0);
    s.runner_key_cb(
        true,
        vec![key_str(BOB_RUNNER)],
        vec![key_str(BOB_RUNNER)],
        key_str(CAROL_RUNNER),
    );
    assert_eq!(s.runner(key(CAROL_RUNNER)).heartbeat()["ok"], true);
    let card = s
        .by_1y(BOB)
        .list_for_sale(U128(1_000 * USD), CODE_HASH.into(), None);
    assert_eq!(card["schema"], "listing.v1");
}

/// A25: a failed rotation batch restores the old key; nothing is bricked.
#[test]
fn a25_failed_rotation_restores_old_key() {
    let mut s = sold_translator();
    s.by_1y(BOB).set_runner_key(key(CAROL_RUNNER));
    s.runner_key_cb(
        false,
        vec![key_str(BOB_RUNNER)],
        vec![key_str(BOB_RUNNER)],
        key_str(CAROL_RUNNER),
    );
    assert_eq!(s.c.known_keys(), vec![key_str(BOB_RUNNER)]);
    assert_eq!(s.runner(key(BOB_RUNNER)).heartbeat()["ok"], true);
    assert!(!s.m().frozen);
}

/// A25 (review C1c): same gap on nft_transfer to an alt account.
#[test]
fn a25_transfer_to_alt_gap_does_not_freeze() {
    let mut s = sold_translator();
    s.by_1y(BOB)
        .nft_transfer(acc(CAROL), DEED_ID.into(), None, None);
    let msg = panic_text(|| {
        s.runner(key(BOB_RUNNER)).heartbeat();
    });
    assert_eq!(msg, "NotRunner");
    assert!(!s.m().frozen);
    assert_eq!(s.c.get_claimable(acc(BOB)).0, 0);
    assert_eq!(s.m().escrow_left(), 300 * USD);
}

/// A26 (review H1): the seller cannot choose the admin, the token or the
/// signer list at init; they are pinned in the code and shown in get_status.
#[test]
fn a26_admin_token_signers_pinned() {
    let mut s = Sim::new(CHEAT, ALICE);
    let st = s.by(ALICE).get_status();
    assert_eq!(st["admin"], REGISTRY_ADMIN);
    assert_eq!(st["token"], PAYMENT_TOKEN);
    // The seller cannot allowlist their own signer.
    let msg = panic_text(|| {
        s.by(ALICE)
            .add_signer("0x1563915e194d8cfba1943570603f7606a3115508".into());
    });
    assert_eq!(msg, "NotAdmin");
    // Only the pinned token's ft_on_transfer is accepted.
    let msg = panic_text(|| {
        s.by(ALICE)
            .ft_on_transfer(acc(ALICE), U128(USD), String::new());
    });
    assert_eq!(msg, "UnknownToken");
}

/// A27 (review F3): one signature for a 12.00 job by p1 does not verify a
/// 100,000.00 payment from another account, nor a bigger payment by p1.
#[test]
fn a27_verified_bound_to_payer_and_amount() {
    let mut s = Sim::new(AGENT, ALICE);
    s.ft("sock-puppet.testnet", 100_000 * USD, &job_msg(&job(0)), 100);
    assert_eq!(events("payment")[0]["verify"], "WrongPayer");
    s.ft(PAYERS[0], 100_000 * USD, &job_msg(&job(0)), 100);
    assert_eq!(events("payment")[0]["verify"], "WrongAmount");
    let e = s.c.get_earnings();
    assert_eq!(e.verified, 0);
    // The signature is still unused: the real payment verifies.
    s.ft(PAYERS[0], 12 * USD, &job_msg(&job(0)), 100);
    assert_eq!(s.c.get_earnings().verified, 12 * USD);
}

/// A28 (review F4): selling, cancelling, rotating keys and setting limits
/// need 1 yoctoNEAR, so a function-call key on the owner's account (which
/// cannot attach a deposit) cannot call them.
#[test]
fn a28_owner_methods_need_full_access_key() {
    let mut s = translator_world();
    let t = agent_core::Terms {
        earnout_bps: 0,
        share_bps: 1,
        floor_bps: 10_000,
        window_secs: 60,
    };
    type Call = Box<dyn Fn(&mut Sim)>;
    let calls: Vec<(&str, Call)> = vec![
        (
            "list_for_sale",
            Box::new(move |s: &mut Sim| {
                s.by(ALICE)
                    .list_for_sale(U128(USD), CODE_HASH.into(), Some(t));
            }),
        ),
        (
            "set_runner_key",
            Box::new(|s: &mut Sim| {
                s.by(ALICE).set_runner_key(key(9));
            }),
        ),
        (
            "set_limits",
            Box::new(|s: &mut Sim| {
                s.by(ALICE).set_limits(demo_limits());
            }),
        ),
        (
            "nft_transfer",
            Box::new(|s: &mut Sim| {
                s.by(ALICE)
                    .nft_transfer(acc(MALLORY), DEED_ID.into(), None, None);
            }),
        ),
    ];
    for (name, f) in &calls {
        let msg = panic_text(|| f(&mut s));
        assert!(msg.contains("1 yoctoNEAR"), "{name}: {msg}");
    }
    s.by_1y(ALICE)
        .list_for_sale(U128(1_000 * USD), CODE_HASH.into(), None);
    let msg = panic_text(|| {
        s.by(ALICE).cancel_listing();
    });
    assert!(msg.contains("1 yoctoNEAR"), "cancel_listing: {msg}");
    assert_eq!(s.m().status, agent_core::Status::Listed);
}

/// A29 (review F6): the seller's unwithdrawn revenue is settled to the
/// seller at the sale; it does not pass to the buyer with the deed.
#[test]
fn a29_seller_revenue_settled_at_sale() {
    let mut s = sold_translator();
    assert_eq!(s.m().owner_free, 0);
    let msg = panic_text(|| {
        s.by_1y(BOB).withdraw(U128(500 * USD), None);
    });
    assert_eq!(msg, "InsufficientFunds");
    assert_eq!(s.c.get_claimable(acc(ALICE)).0, 500 * USD);
    assert_eq!(events("sale").len(), 0, "events of the last call only");
    assert!(s.m().solvent());
}

/// A30 (review F7): `paid_to` never silently stops tracking. After 1000 new
/// destinations, a withdrawal to a new account is refused, so a round-trip
/// payer can never go unflagged; withdrawing to the owner still works.
#[test]
fn a30_round_trip_flag_does_not_saturate() {
    let mut s = translator_world();
    for i in 0..agent_core::MAX_PAID_TO {
        s.by_1y(ALICE)
            .withdraw(U128(1), Some(acc(&format!("junk{i}.testnet"))));
    }
    let msg = panic_text(|| {
        s.by_1y(ALICE)
            .withdraw(U128(10 * USD), Some(acc("friend.testnet")));
    });
    assert_eq!(msg, "PaidToFull");
    // Someone already paid is still flagged when paying back.
    s.pay("junk7.testnet", 10 * USD);
    assert_eq!(s.c.get_earnings().flagged_round_trip, 10 * USD);
    // The owner can always withdraw to themselves (I10).
    s.by_1y(ALICE).withdraw(U128(USD), None);
}
