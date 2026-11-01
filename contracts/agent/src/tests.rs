// Promises returned by contract methods are dropped on purpose: in the mocked
// runtime dropping schedules them, and the tests inspect the created receipts.
#![allow(unused_must_use)]

//! Contract unit tests in the mocked runtime, named after SCENARIOS.md ids.
//! The `sbx` / `net` parts of a scenario (real key errors, real token
//! refunds) run in near-sandbox or on testnet; see README "What runs where".

use crate::sim::*;
use crate::*;
use agent_core::Status;

fn refused(reason: &str) -> bool {
    events("refused").iter().any(|e| e["reason"] == reason)
}

fn panics<F: FnOnce()>(f: F) -> bool {
    std::panic::catch_unwind(std::panic::AssertUnwindSafe(f)).is_err()
}

/// The contract's panic text of `f` (`env::panic_str` argument), or "" if it
/// did not panic. The mocked runtime wraps it in `GuestPanic { panic_msg: .. }`.
fn panic_msg<F: FnOnce()>(f: F) -> String {
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

#[test]
fn s1_list_clean() {
    let mut s = translator_world();
    assert_eq!(s.c.known_keys(), vec![key_str(ALICE_RUNNER)]);
    let card = s
        .by(ALICE)
        .list_for_sale(U128(1_000 * USD), CODE_HASH.into(), None);
    assert_eq!(card["schema"], "listing.v1");
    assert_eq!(card["listing_version"], 1);
    assert_eq!(card["price"], "1000000000");
    assert_eq!(card["paid_now"], "700000000");
    assert_eq!(card["escrow"], "300000000");
    assert_eq!(card["code_hash"], CODE_HASH);
    assert_eq!(card["known_keys_hash"], s.c.known_keys_hash());
    assert_eq!(s.m().status, Status::Listed);
    assert_eq!(events("listed").len(), 1);
}

#[test]
fn s1_runner_key_is_function_call_only() {
    let mut s = Sim::new(AGENT, ALICE);
    s.by(ALICE).set_runner_key(key(ALICE_RUNNER));
    assert_eq!(
        key_actions(AGENT),
        vec![("add_fc".to_string(), key_str(ALICE_RUNNER))]
    );
}

#[test]
fn s2_meter_fixed_history() {
    let s = translator_world();
    let e = s.c.get_earnings();
    assert_eq!(e.gross, 500 * USD);
    assert_eq!(e.counted, 360 * USD);
    assert_eq!(e.flagged, 140 * USD);
    assert_eq!(e.flagged_self, 100 * USD);
    assert_eq!(e.flagged_seller_subaccount, 40 * USD);
    assert_eq!(e.counted_payers, 6);
    assert_eq!(e.payments, 33);
    // Per-day buckets: days 0-4 each 72.00 counted; day 5 140.00 flagged.
    assert_eq!(e.days.len(), 6);
    for d in &e.days[..5] {
        assert_eq!((d.gross, d.counted, d.payments), (72 * USD, 72 * USD, 6));
    }
    assert_eq!((e.days[5].gross, e.days[5].counted), (140 * USD, 0));
    // 10 payments carry a signature from the allowlisted (test) signer.
    assert_eq!(e.verified, 120 * USD);
    assert_eq!(e.days[0].verified, 72 * USD);
    assert_eq!(e.days[1].verified, 48 * USD);
}

#[test]
fn s4_side_by_side() {
    let t = translator_world().c.get_earnings();
    let c = cheat_world().c.get_earnings();
    assert_eq!((t.gross, t.counted), (500 * USD, 360 * USD));
    assert_eq!((c.gross, c.counted, c.verified), (500 * USD, 0, 0));
    assert_eq!(c.flagged_self, 300 * USD);
    assert_eq!(c.flagged_seller_subaccount, 200 * USD);
}

#[test]
fn s5_buy_one_step() {
    let mut s = listed_translator();
    assert_eq!(s.c.known_keys(), vec![key_str(ALICE_RUNNER)]);
    // bob's ft_transfer_call(1000, buy msg) -> a promise (the key batch).
    assert_eq!(s.buy(BOB, 1_000 * USD, BOB_RUNNER), None);
    assert_eq!(s.m().status, Status::Settling);
    // One batch on the agent: delete alice's runner key, add bob's (FC only).
    assert_eq!(
        key_actions(AGENT),
        vec![
            ("delete".to_string(), key_str(ALICE_RUNNER)),
            ("add_fc".to_string(), key_str(BOB_RUNNER)),
        ]
    );
    // Callback after the batch succeeded.
    assert_eq!(s.callback(true).on_handover(1).0, 0);
    assert_eq!(ft_transfers(), vec![(ALICE.to_string(), 700 * USD)]);
    let sale = &events("sale")[0];
    for f in [
        "sale_id",
        "seller",
        "buyer",
        "price",
        "paid_now",
        "escrow",
        "owner_epoch",
        "window_end",
        "known_keys",
    ] {
        assert!(!sale[f].is_null(), "sale event lacks {f}");
    }
    assert_eq!(sale["escrow"], "300000000");
    assert_eq!(
        s.c.nft_token(DEED_ID.into()).unwrap().owner_id.as_str(),
        BOB
    );
    assert_eq!(s.c.known_keys(), vec![key_str(BOB_RUNNER)]);
    assert_eq!(s.m().status, Status::Sold);
    assert_eq!(s.m().owner_epoch, 2);
    assert_eq!(s.m().escrow_left(), 300 * USD);
    assert!(s.m().solvent());
}

#[test]
fn e1_stale_listing_version() {
    let mut s = listed_translator();
    let h = s.c.known_keys_hash();
    let r = s.ft(BOB, 1_000 * USD, &Sim::buy_msg(0, &h, BOB_RUNNER), 300);
    assert_eq!(r, Some(1_000 * USD));
    assert!(refused("ListingChanged"));
    assert_eq!(s.m().status, Status::Listed);
    assert!(key_actions(AGENT).is_empty());
}

#[test]
fn a7_terms_changed_after_scan() {
    let mut s = listed_translator();
    let seen_version = s.m().listing_version;
    let h = s.c.known_keys_hash();
    // The seller re-lists at another price after bob scanned.
    s.by(ALICE).cancel_listing();
    s.by(ALICE)
        .list_for_sale(U128(1_500 * USD), CODE_HASH.into(), None);
    let r = s.ft(
        BOB,
        1_500 * USD,
        &Sim::buy_msg(seen_version, &h, BOB_RUNNER),
        300,
    );
    assert_eq!(r, Some(1_500 * USD));
    assert!(refused("ListingChanged"));
}

#[test]
fn e2_wrong_known_keys_hash() {
    let mut s = listed_translator();
    let v = s.m().listing_version;
    let r = s.ft(BOB, 1_000 * USD, &Sim::buy_msg(v, "00ff", BOB_RUNNER), 300);
    assert_eq!(r, Some(1_000 * USD));
    assert!(refused("KeysChanged"));
}

#[test]
fn e3_underpay_refused() {
    let mut s = listed_translator();
    assert_eq!(s.buy(BOB, 999_990_000, BOB_RUNNER), Some(999_990_000));
    assert!(refused("Underpaid"));
    assert_eq!(s.m().status, Status::Listed);
}

#[test]
fn e3_overpay_returns_change() {
    let mut s = listed_translator();
    assert_eq!(s.buy(BOB, 1_000_500_000, BOB_RUNNER), None);
    assert_eq!(s.callback(true).on_handover(1).0, 500_000);
    // Held: 300 escrow + 500 pre-sale revenue; the 700 payout is in flight.
    assert_eq!(s.m().held, 800 * USD);
}

#[test]
fn e4_second_buy_while_settling() {
    let mut s = listed_translator();
    assert_eq!(s.buy(BOB, 1_000 * USD, BOB_RUNNER), None);
    assert_eq!(s.buy(CAROL, 1_000 * USD, CAROL_RUNNER), Some(1_000 * USD));
    assert!(refused("SaleInProgress"));
}

#[test]
fn e5_rollback() {
    let mut s = listed_translator();
    let before = s.m().clone();
    assert_eq!(s.buy(BOB, 1_000 * USD, BOB_RUNNER), None);
    // The key batch failed (e.g. bob's key was already on the account).
    let unused = s.callback(false).on_handover(1);
    assert_eq!(unused.0, 1_000 * USD, "bob refunded in full");
    assert!(ft_transfers().is_empty(), "no seller payout");
    assert_eq!(events("sale_rolled_back").len(), 1);
    let mut expect = before;
    expect.next_sale_id = 2;
    assert_eq!(s.m(), &expect, "state exactly as before the buy");
    assert_eq!(s.c.known_keys(), vec![key_str(ALICE_RUNNER)]);
}

#[test]
fn e6_payout_fallback() {
    let mut s = listed_translator();
    s.buy(BOB, 1_000 * USD, BOB_RUNNER);
    s.callback(true).on_handover(1);
    // alice is not registered on the token: the push fails.
    s.callback(false)
        .on_transfer_out(OutKind::SellerPayout, acc(ALICE), U128(700 * USD));
    assert_eq!(s.c.get_claimable(acc(ALICE)).0, 700 * USD);
    assert!(s.m().solvent());
    s.by(ALICE).claim();
    assert_eq!(ft_transfers(), vec![(ALICE.to_string(), 700 * USD)]);
    assert_eq!(s.c.get_claimable(acc(ALICE)).0, 0);
}

#[test]
fn e7_not_enough_gas() {
    let mut s = listed_translator();
    let v = s.m().listing_version;
    let h = s.c.known_keys_hash();
    let r = s.ft(BOB, 1_000 * USD, &Sim::buy_msg(v, &h, BOB_RUNNER), 50);
    assert_eq!(r, Some(1_000 * USD));
    assert!(refused("NotEnoughGas"));
    assert_eq!(s.m().status, Status::Listed);
    assert!(key_actions(AGENT).is_empty());
}

#[test]
#[should_panic(expected = "UnknownToken")]
fn e8_a14_direct_call_by_non_token() {
    let mut s = translator_world();
    s.by(MALLORY)
        .ft_on_transfer(acc(MALLORY), U128(1_000 * USD), "".into());
}

#[test]
#[should_panic(expected = "UnknownToken")]
fn e9_token_not_on_list() {
    // A real but unlisted token gets a panic; its ft_resolve_transfer then
    // refunds the sender in full (NEP-141).
    let mut s = translator_world();
    s.by("othertoken.testnet")
        .ft_on_transfer(acc(BOB), U128(5 * USD), "".into());
}

#[test]
fn e10_malformed_or_huge_msg() {
    let mut s = listed_translator();
    let before = s.m().clone();
    for msg in [
        "{",
        "{\"kind\":\"steal\"}",
        "{\"kind\":\"buy\",\"listing_version\":\"x\"}",
        "{\"kind\":\"buy\",\"listing_version\":1,\"known_keys_hash\":\"\",\"buyer_runner_pk\":\"ed25519:nope\"}",
        "null",
    ] {
        assert_eq!(s.ft(BOB, 7 * USD, msg, 300), Some(7 * USD), "msg {msg}");
    }
    let huge = format!(
        "{{\"kind\":\"pay\",\"job_id\":\"{}\"}}",
        "a".repeat(MAX_MSG_LEN)
    );
    assert_eq!(s.ft(BOB, 7 * USD, &huge, 300), Some(7 * USD));
    assert_eq!(s.m(), &before);
}

#[test]
fn e15_dust_refunded() {
    let mut s = translator_world();
    let payers = s.m().meter.payers.len();
    assert_eq!(s.pay("dust.testnet", 50_000), Some(50_000));
    assert!(refused("Dust"));
    assert_eq!(s.m().meter.payers.len(), payers);
}

#[test]
fn e17_withdraw_near_everything_above_reserve() {
    let mut s = translator_world();
    // Measured in the same context as the call (the 1 yocto deposit counts
    // toward the account balance on NEAR).
    let max = s.by_1y(ALICE).near_withdrawable();
    assert!(max.as_yoctonear() > 0);
    s.by_1y(ALICE).withdraw_near(max);
}

#[test]
#[should_panic(expected = "StorageReserve")]
fn e17_withdraw_near_over_reserve_refused() {
    let mut s = translator_world();
    // Measured in the same context as the call (the 1 yocto deposit counts
    // toward the account balance on NEAR).
    let max = s.by_1y(ALICE).near_withdrawable();
    s.by_1y(ALICE)
        .withdraw_near(max.saturating_add(NearToken::from_yoctonear(1)));
}

#[test]
fn e21_finalize_twice_or_by_anyone() {
    let mut s = sold_translator();
    s.advance(30 * DAY);
    let a = s.by(MALLORY).finalize_earnout(1);
    let b = s.by(CAROL).finalize_earnout(1);
    assert_eq!(a, b);
    assert_eq!(a["released"], "60000000");
    assert_eq!(a["returned"], "240000000");
    assert_eq!(s.c.get_claimable(acc(BOB)).0, 240 * USD);
}

#[test]
fn e25_approval_blocks_listing_and_is_cleared_on_transfer() {
    let mut s = translator_world();
    s.by_1y(ALICE)
        .nft_approve(DEED_ID.into(), acc(MALLORY), None);
    assert!(panics(|| {
        s.by(ALICE)
            .list_for_sale(U128(1_000 * USD), CODE_HASH.into(), None);
    }));
    assert!(s.c.nft_is_approved(DEED_ID.into(), acc(MALLORY), None));
    s.by_1y(ALICE)
        .nft_transfer(acc(CAROL), DEED_ID.into(), None, None);
    assert!(!s.c.nft_is_approved(DEED_ID.into(), acc(MALLORY), None));
    assert!(s.m().approvals.is_empty());
}

#[test]
#[should_panic(expected = "OpenApproval")]
fn a8_open_approval_list_refused() {
    let mut s = translator_world();
    s.by_1y(ALICE)
        .nft_approve(DEED_ID.into(), acc(MALLORY), None);
    s.by(ALICE)
        .list_for_sale(U128(1_000 * USD), CODE_HASH.into(), None);
}

#[test]
#[should_panic(expected = "SaleInProgress")]
fn e26_cancel_during_settling() {
    let mut s = listed_translator();
    s.buy(BOB, 1_000 * USD, BOB_RUNNER);
    s.by(ALICE).cancel_listing();
}

#[test]
#[should_panic(expected = "SaleInProgress")]
fn e27_runner_during_settling() {
    let mut s = listed_translator();
    s.buy(BOB, 1_000 * USD, BOB_RUNNER);
    s.runner(key(ALICE_RUNNER)).heartbeat();
}

#[test]
fn a10_a11_a12_wash_flags() {
    let mut s = translator_world();
    // A12: the agent pays p7 (owner withdraws revenue to p7), p7 pays back.
    s.by_1y(ALICE)
        .withdraw(U128(10 * USD), Some(acc("p7.testnet")));
    s.callback(true)
        .on_transfer_out(OutKind::OwnerWithdraw, acc("p7.testnet"), U128(10 * USD));
    s.pay("p7.testnet", 10 * USD);
    let p = &events("payment")[0];
    assert_eq!(p["flags"]["round_trip"], true);
    assert_eq!(p["counted"], false);
    // A10: self-payment.
    s.pay(ALICE, 5 * USD);
    assert_eq!(events("payment")[0]["flags"]["self"], true);
    // A11: seller sub-account.
    s.pay(X_ALICE, 5 * USD);
    assert_eq!(events("payment")[0]["flags"]["seller_subaccount"], true);
    // A sub-account of the agent itself is flagged too.
    s.pay("x.translator.testnet", 5 * USD);
    assert_eq!(events("payment")[0]["counted"], false);
    let e = s.c.get_earnings();
    assert_eq!(e.counted, 360 * USD);
    assert_eq!(e.flagged_round_trip, 10 * USD);
}

#[test]
fn s7_earnout_partial_via_contract() {
    let mut s = sold_translator();
    for i in 1..=4u64 {
        s.now = T0 + (10 + i) * DAY;
        s.pay(PAYERS[0], 25 * USD);
        assert_eq!(events("earnout_release")[0]["amount"], "12500000");
    }
    assert_eq!(s.c.get_claimable(acc(ALICE)).0, 50 * USD);
    s.now = T0 + 41 * DAY;
    s.by(MALLORY).finalize_earnout(1);
    assert_eq!(s.c.get_claimable(acc(ALICE)).0, 60 * USD);
    assert_eq!(s.c.get_claimable(acc(BOB)).0, 240 * USD);
}

#[test]
fn s10_wash_after_sale_via_contract() {
    let mut s = sold_translator();
    s.pay(ALICE, 100 * USD);
    assert_eq!(events("payment")[0]["flags"]["self"], true);
    assert!(events("earnout_release").is_empty());
    assert_eq!(s.c.get_claimable(acc(ALICE)).0, 0);
}

#[test]
fn a16_e24_deed_moved_outside_buy_deletes_runner_keys() {
    let mut s = translator_world();
    s.by_1y(ALICE)
        .nft_transfer(acc(MALLORY), DEED_ID.into(), None, None);
    assert_eq!(
        key_actions(AGENT),
        vec![("delete".to_string(), key_str(ALICE_RUNNER))]
    );
    assert!(s.c.known_keys().is_empty());
    assert!(s.m().paused);
    assert_eq!(
        s.c.nft_token(DEED_ID.into()).unwrap().owner_id.as_str(),
        MALLORY
    );
    // The new owner sets a runner key; the agent runs again.
    s.by(MALLORY).set_runner_key(key(CAROL_RUNNER));
    s.callback(true)
        .on_runner_key(vec![], key_str(CAROL_RUNNER));
    assert_eq!(s.runner(key(CAROL_RUNNER)).heartbeat()["ok"], true);
}

#[test]
fn a16_old_runner_key_after_transfer_proves_backdoor() {
    // The deleted key cannot sign on chain; if a self-call still arrives with
    // it, the key was never really gone: freeze (A23 path).
    let mut s = translator_world();
    s.by_1y(ALICE)
        .nft_transfer(acc(MALLORY), DEED_ID.into(), None, None);
    assert_eq!(s.runner(key(ALICE_RUNNER)).heartbeat()["frozen"], true);
}

#[test]
fn a17_old_runner_key_removed_by_sale() {
    // Unit part: the sale's batch deletes alice's runner key and the contract
    // no longer lists it. The live `AccessKeyNotFound` is sbx/net.
    let mut s = listed_translator();
    s.buy(BOB, 1_000 * USD, BOB_RUNNER);
    assert!(key_actions(AGENT).contains(&("delete".to_string(), key_str(ALICE_RUNNER))));
    s.callback(true).on_handover(1);
    assert!(!s.c.known_keys().contains(&key_str(ALICE_RUNNER)));
}

#[test]
#[should_panic(expected = "NotOwner")]
fn a19_old_seller_withdraw_refused() {
    let mut s = sold_translator();
    s.by_1y(ALICE).withdraw(U128(USD), None);
}

#[test]
#[should_panic(expected = "NotOwner")]
fn a22_old_seller_moves_deed_back_refused() {
    let mut s = sold_translator();
    s.by_1y(ALICE)
        .nft_transfer(acc(ALICE), DEED_ID.into(), None, None);
}

#[test]
fn a23_hidden_key_freezes_and_returns_escrow() {
    let mut s = sold_translator();
    s.now = T0 + 11 * DAY;
    s.pay(PAYERS[0], 20 * USD); // releases 10.00 to alice
    let r = s.runner(key(HIDDEN_KEY)).heartbeat();
    assert_eq!(r["frozen"], true);
    let ev = &events("backdoor_proven")[0];
    assert_eq!(ev["returned"][0]["buyer"], BOB);
    assert_eq!(ev["returned"][0]["amount"], "290000000");
    assert!(s.m().frozen);
    assert_eq!(s.c.get_claimable(acc(BOB)).0, 290 * USD);
    // bob can still take his revenue out (I10).
    s.by_1y(BOB).withdraw(U128(20 * USD), None);
}

#[test]
fn a23_known_runner_key_is_not_frozen() {
    let mut s = sold_translator();
    assert_eq!(s.runner(key(BOB_RUNNER)).heartbeat()["ok"], true);
    assert!(!s.m().frozen);
}

#[test]
fn s19_dynamic_metadata() {
    let mut s = translator_world();
    let extra = |s: &Sim| -> Value {
        let t = s.c.nft_token(DEED_ID.into()).unwrap();
        serde_json::from_str(&t.metadata.unwrap().extra.unwrap()).unwrap()
    };
    let x = extra(&s);
    assert_eq!(x["schema"], "deed-extra.v1");
    assert_eq!(x["earnings"]["counted"], "360000000");
    s.pay(PAYERS[1], 10 * USD);
    assert_eq!(extra(&s)["earnings"]["counted"], "370000000");
    assert_eq!(s.c.nft_metadata().spec, "nft-1.0.0");
    assert_eq!(s.c.nft_total_supply().0, 1);
    assert_eq!(s.c.nft_supply_for_owner(acc(ALICE)).0, 1);
    assert_eq!(s.c.nft_supply_for_owner(acc(BOB)).0, 0);
}

#[test]
fn s15_resale_via_contract() {
    let mut s = sold_translator();
    s.by(BOB)
        .list_for_sale(U128(2_000 * USD), CODE_HASH.into(), None);
    assert_eq!(s.buy(CAROL, 2_000 * USD, CAROL_RUNNER), None);
    assert_eq!(
        key_actions(AGENT),
        vec![
            ("delete".to_string(), key_str(BOB_RUNNER)),
            ("add_fc".to_string(), key_str(CAROL_RUNNER)),
        ]
    );
    s.callback(true).on_handover(2);
    assert_eq!(s.m().owner, CAROL);
    assert_eq!(s.m().owner_epoch, 3);
    // History travels: the meter still shows the pre-sale numbers.
    assert_eq!(s.c.get_earnings().counted, 360 * USD);
    // alice's earn-out keeps releasing first.
    s.now = T0 + 12 * DAY;
    s.pay(PAYERS[2], 10 * USD);
    assert_eq!(events("earnout_release")[0]["seller"], ALICE);
    // Both old owners are locked out of owner methods.
    for old in [ALICE, BOB] {
        assert!(panics(|| {
            s.by_1y(old).withdraw(U128(USD), None);
        }));
    }
}

/// I4 (unit form): every owner method refuses every non-owner, and a refused
/// call changes nothing.
#[test]
fn i4_access_matrix_owner_methods() {
    type Call = fn(&mut Sim, &str);
    let calls: Vec<(&str, Call)> = vec![
        ("set_runner_key", |s, w| {
            s.by(w).set_runner_key(key(77));
        }),
        ("list_for_sale", |s, w| {
            s.by(w).list_for_sale(U128(USD), CODE_HASH.into(), None);
        }),
        ("cancel_listing", |s, w| s.by(w).cancel_listing()),
        ("withdraw", |s, w| {
            s.by_1y(w).withdraw(U128(USD), None);
        }),
        ("withdraw_near", |s, w| {
            s.by_1y(w).withdraw_near(NearToken::from_yoctonear(1));
        }),
        ("nft_transfer", |s, w| {
            s.by_1y(w)
                .nft_transfer(acc(MALLORY), DEED_ID.into(), None, None)
        }),
        ("nft_approve", |s, w| {
            s.by_1y(w).nft_approve(DEED_ID.into(), acc(MALLORY), None);
        }),
        ("nft_revoke_all", |s, w| {
            s.by_1y(w).nft_revoke_all(DEED_ID.into())
        }),
        ("heartbeat", |s, w| {
            s.by(w).heartbeat();
        }),
    ];
    // After the sale bob owns the agent; everyone else must be refused.
    for who in [ALICE, CAROL, MALLORY, ADMIN, TOKEN] {
        for (name, f) in &calls {
            let mut s = sold_translator();
            if *name == "cancel_listing" {
                s.by(BOB)
                    .list_for_sale(U128(2_000 * USD), CODE_HASH.into(), None);
            }
            let before = s.m().clone();
            assert!(panics(|| f(&mut s, who)), "{who} could call {name}");
            assert_eq!(s.m(), &before, "{who}/{name} changed state");
        }
    }
}

// ------------------------------------------------------------ NEAR Verified

fn verify_note() -> String {
    events("payment")[0]["verify"].as_str().unwrap().to_string()
}

#[test]
fn s3_verified_badge() {
    let mut s = Sim::new(AGENT, ALICE);
    let e = job(0);
    assert_eq!(s.ft(PAYERS[0], 12 * USD, &job_msg(&e), 100), Some(0));
    assert_eq!(events("payment")[0]["verified"], true);
    assert_eq!(verify_note(), format!("ok:{}", test_signer()));
    // Same amount without a signature: counts, not verified.
    s.ft(
        PAYERS[1],
        12 * USD,
        "{\"kind\":\"pay\",\"job_id\":\"job-2\"}",
        100,
    );
    assert_eq!(events("payment")[0]["verified"], false);
    assert_eq!(verify_note(), "NoProof");
    let x = s.c.get_earnings();
    assert_eq!(
        (x.gross, x.counted, x.verified),
        (24 * USD, 24 * USD, 12 * USD)
    );
}

#[test]
fn e11_same_signature_twice() {
    let mut s = Sim::new(AGENT, ALICE);
    let msg = job_msg(&job(0));
    s.ft(PAYERS[0], 12 * USD, &msg, 100);
    s.ft(PAYERS[1], 12 * USD, &msg, 100);
    assert_eq!(verify_note(), "SignatureUsed");
    let x = s.c.get_earnings();
    assert_eq!((x.counted, x.verified), (24 * USD, 12 * USD));
}

#[test]
fn e11_flipped_v_is_the_same_signature() {
    let mut s = Sim::new(AGENT, ALICE);
    let e = job(0);
    s.ft(PAYERS[0], 12 * USD, &job_msg(&e), 100);
    // Same r||s with v written as 0/1 instead of 27/28.
    let mut e2 = e.clone();
    let sig = e["signature"].as_str().unwrap();
    let v = u8::from_str_radix(&sig[128..], 16).unwrap() - 27;
    e2["signature"] = json!(format!("{}{:02x}", &sig[..128], v));
    s.ft(PAYERS[1], 12 * USD, &job_msg(&e2), 100);
    assert_eq!(verify_note(), "SignatureUsed");
}

#[test]
fn e12_signature_for_another_agent() {
    let mut s = Sim::new(AGENT, ALICE);
    s.ft(
        PAYERS[0],
        12 * USD,
        &job_msg(&fixtures()["wrong_agent"]),
        100,
    );
    assert_eq!(verify_note(), "WrongAgent");
    assert_eq!(events("payment")[0]["counted"], true);
    // Right agent, but the payment names another job.
    let msg = json!({"kind": "pay", "job_id": "job-99", "nearai": proof(&job(0))}).to_string();
    s.ft(PAYERS[1], 12 * USD, &msg, 100);
    assert_eq!(verify_note(), "WrongJob");
    assert_eq!(s.c.get_earnings().verified, 0);
}

#[test]
fn e13_forged_wrong_signer_bad_v_bad_length() {
    let mut s = Sim::new(AGENT, ALICE);
    // Signed by a key nobody allowlisted.
    s.ft(PAYERS[0], 12 * USD, &job_msg(&fixtures()["rogue"]), 100);
    assert_eq!(verify_note(), "UnknownSigner");
    let e = job(0);
    let sig = e["signature"].as_str().unwrap().to_string();
    let cases = [
        (format!("{}05", &sig[..128]), "BadSignature"), // v not in {0,1,27,28}
        (sig[..128].to_string(), "BadSignature"),       // 64 bytes
        ("zz".repeat(65), "BadSignature"),              // not hex
    ];
    for (bad, want) in cases {
        let mut x = e.clone();
        x["signature"] = json!(bad);
        s.ft(PAYERS[1], 12 * USD, &job_msg(&x), 100);
        assert_eq!(verify_note(), want);
    }
    // Tampered response hash: recovers some other address.
    let mut x = e.clone();
    x["response_sha256"] = json!("0".repeat(64));
    s.ft(PAYERS[2], 12 * USD, &job_msg(&x), 100);
    assert_eq!(verify_note(), "UnknownSigner");
    let earn = s.c.get_earnings();
    assert_eq!(earn.verified, 0);
    // 5 payments, all counted, none verified.
    assert_eq!(earn.counted, 5 * 12 * USD);
}

#[test]
fn e14_signer_rotation() {
    let mut s = Sim::new(AGENT, ALICE);
    s.ft(PAYERS[0], 12 * USD, &job_msg(&job(0)), 100);
    let rogue = fixtures()["rogue_signer"].as_str().unwrap().to_string();
    s.by(ADMIN).add_signer(rogue.clone());
    s.by(ADMIN).remove_signer(test_signer());
    assert_eq!(s.c.get_signers(), vec![rogue]);
    // Already-verified payment stays verified.
    assert_eq!(s.c.get_earnings().verified, 12 * USD);
    // A new payment signed by the removed signer is not verified.
    s.ft(PAYERS[1], 12 * USD, &job_msg(&job(1)), 100);
    assert_eq!(verify_note(), "UnknownSigner");
    // The newly added signer verifies.
    s.ft(PAYERS[2], 12 * USD, &job_msg(&fixtures()["rogue"]), 100);
    assert_eq!(events("payment")[0]["verified"], true);
    assert_eq!(s.c.get_earnings().verified, 24 * USD);
}

#[test]
fn a13_replayed_or_forged_not_verified_flagged_not_verified() {
    // A self-payment with a valid signature is neither counted nor verified (I9).
    let mut s = Sim::new(AGENT, ALICE);
    s.ft(ALICE, 12 * USD, &job_msg(&job(0)), 100);
    assert_eq!(events("payment")[0]["verified"], false);
    // ...and does not burn the signature.
    s.ft(PAYERS[0], 12 * USD, &job_msg(&job(0)), 100);
    assert_eq!(events("payment")[0]["verified"], true);
}

#[test]
fn signer_list_is_admin_only() {
    let mut s = translator_world();
    for who in [ALICE, BOB, MALLORY] {
        assert!(panics(|| s.by(who).add_signer(test_signer())));
        assert!(panics(|| s.by(who).remove_signer(test_signer())));
    }
    assert!(panics(|| s.by(ADMIN).add_signer("0x1234".into())));
    assert_eq!(s.c.get_signers(), vec![test_signer()]);
}

// ------------------------------------------------ feature 12: limits

#[test]
fn s11_split_and_procure() {
    let mut s = sold_translator();
    let alice_before = s.c.get_claimable(acc(ALICE)).0;
    // Beat 1: the owner sets the limits.
    let v = s.by_1y(BOB).set_limits(demo_limits());
    assert_eq!(v["schema"], "limits.v1");
    assert_eq!(v["daily_cap"], "50000000");
    assert_eq!(events("limits_set").len(), 1);
    // A job paid 25.00 splits 20.00 owner / 5.00 treasury; the seller's share
    // is the earn-out release (50% of the payment, from escrow).
    let free0 = s.m().owner_free;
    assert_eq!(
        s.ft(PAYERS[0], 25 * USD, r#"{"kind":"pay","job_id":"j1"}"#, 100),
        Some(0)
    );
    let sp = &events("split")[0];
    assert_eq!(sp["to_owner"], "20000000");
    assert_eq!(sp["to_treasury"], "5000000");
    assert_eq!(sp["seller_earnout_release"], "12500000");
    assert_eq!(s.m().owner_free - free0, 20 * USD);
    assert_eq!(s.m().treasury, 5 * USD);
    assert_eq!(s.c.get_claimable(acc(ALICE)).0 - alice_before, 12_500_000);
    assert_eq!(
        s.ft(PAYERS[1], 25 * USD, r#"{"kind":"pay","job_id":"j2"}"#, 100),
        Some(0)
    );
    assert_eq!(s.m().treasury, 10 * USD);
    // Beat 2: the runner pays summarizer 8.00 for a sub-task: allowed.
    let r = s
        .runner(key(BOB_RUNNER))
        .pay(acc(SUMMARIZER), U128(8 * USD));
    assert_eq!(r["ok"], true);
    assert_eq!(ft_transfers(), vec![(SUMMARIZER.to_string(), 8 * USD)]);
    assert_eq!(s.m().treasury, 2 * USD);
    assert_eq!(s.c.get_limits()["spent_today"], "8000000");
    assert_eq!(s.c.get_limits()["cap_left_today"], "42000000");
    assert!(s.m().solvent());
}

#[test]
fn s12_over_limit() {
    let mut s = business_world();
    s.runner(key(BOB_RUNNER))
        .pay(acc(SUMMARIZER), U128(8 * USD));
    let before = s.m().clone();
    // Beat 3: 45.00 more today would be 53.00 > 50.00.
    let msg = panic_msg(|| {
        s.runner(key(BOB_RUNNER))
            .pay(acc(SUMMARIZER), U128(45 * USD));
    });
    assert_eq!(
        msg,
        "PolicyViolation:DailyCap: 8.00 + 45.00 > daily cap 50.00"
    );
    assert_eq!(s.m(), &before, "balances unchanged");
}

#[test]
fn s12_payee_not_allowed_and_treasury_short() {
    let mut s = business_world();
    let msg = panic_msg(|| {
        s.runner(key(BOB_RUNNER)).pay(acc(MALLORY), U128(USD));
    });
    assert_eq!(
        msg,
        "PolicyViolation:Payee: mallory.testnet is not an allowed payee"
    );
    let msg = panic_msg(|| {
        s.runner(key(BOB_RUNNER))
            .pay(acc(SUMMARIZER), U128(11 * USD));
    });
    assert_eq!(msg, "InsufficientFunds: treasury 10.00 < 11.00");
    assert_eq!(s.m().treasury, 10 * USD);
}

#[test]
fn s13_constraints_owner_only() {
    let mut s = sold_translator();
    s.by_1y(BOB).set_limits(demo_limits());
    assert_eq!(s.m().limits.daily_cap, 50 * USD);
    // The seller cannot change the limits after the sale.
    let msg = panic_msg(|| {
        s.by_1y(ALICE).set_limits(demo_limits());
    });
    assert_eq!(msg, "NotOwner");
    // Neither can the runner (a self-call by the agent).
    let msg = panic_msg(|| {
        let mut c = s.call(AGENT);
        c.signer_pk = key(BOB_RUNNER);
        c.deposit = NearToken::from_yoctonear(1);
        s.env(&c);
        s.c.set_limits(demo_limits());
    });
    assert_eq!(msg, "NotOwner");
    // A function-call key cannot attach the 1 yoctoNEAR set_limits needs.
    assert!(panics(|| {
        s.by(BOB).set_limits(demo_limits());
    }));
}

#[test]
fn s13_limits_refused_while_listed_and_move_with_the_deed() {
    let mut s = translator_world();
    s.by_1y(ALICE).set_limits(demo_limits());
    s.by(ALICE)
        .list_for_sale(U128(1_000 * USD), CODE_HASH.into(), None);
    let msg = panic_msg(|| {
        s.by_1y(ALICE).set_limits(demo_limits());
    });
    assert_eq!(msg, "AlreadyListed");
    assert_eq!(s.buy(BOB, 1_000 * USD, BOB_RUNNER), None);
    let id = s.m().pending.as_ref().unwrap().sale_id;
    s.callback(true).on_handover(id);
    // The limits stay; bob now controls them.
    assert_eq!(s.c.get_limits()["owner"], BOB);
    assert_eq!(s.c.get_limits()["daily_cap"], "50000000");
}

#[test]
fn e18_cap_resets_at_sale() {
    // A15: alice burns the whole daily cap just before the sale.
    let mut s = translator_world();
    s.by_1y(ALICE).set_limits(demo_limits());
    for p in PAYERS.iter().take(5) {
        assert_eq!(s.pay(p, 60 * USD), Some(0));
    }
    assert_eq!(s.m().treasury, 60 * USD);
    s.runner(key(ALICE_RUNNER))
        .pay(acc(SUMMARIZER), U128(50 * USD));
    assert!(panics(|| {
        s.runner(key(ALICE_RUNNER)).pay(acc(SUMMARIZER), U128(USD));
    }));
    s.by(ALICE)
        .list_for_sale(U128(1_000 * USD), CODE_HASH.into(), None);
    assert_eq!(s.buy(BOB, 1_000 * USD, BOB_RUNNER), None);
    let id = s.m().pending.as_ref().unwrap().sale_id;
    s.callback(true).on_handover(id);
    // Same day, new owner epoch: bob's runner starts with the full 50.00.
    assert_eq!(s.c.get_limits()["cap_left_today"], "50000000");
    let r = s
        .runner(key(BOB_RUNNER))
        .pay(acc(SUMMARIZER), U128(10 * USD));
    assert_eq!(r["ok"], true);
}

#[test]
fn e19_cap_resets_by_utc_day() {
    let mut s = business_world();
    s.ft(PAYERS[2], 300 * USD, "", 100);
    s.runner(key(BOB_RUNNER))
        .pay(acc(SUMMARIZER), U128(50 * USD));
    assert!(panics(|| {
        s.runner(key(BOB_RUNNER)).pay(acc(SUMMARIZER), U128(USD));
    }));
    // Next UTC day by block timestamp: the cap is fresh.
    s.now = (s.now / DAY + 1) * DAY;
    assert_eq!(s.by(BOB).get_limits()["cap_left_today"], "50000000");
    // Treasury: 10.00 + 20% of 300.00 - 50.00 = 20.00.
    let r = s
        .runner(key(BOB_RUNNER))
        .pay(acc(SUMMARIZER), U128(20 * USD));
    assert_eq!(r["ok"], true);
    assert_eq!(s.m().treasury, 0);
}

#[test]
fn e33_bad_limits_refused() {
    let mut s = sold_translator();
    let mut bad = vec![];
    let mut l = demo_limits();
    l.split.owner_bps = 7_000; // 70% + 20% != 100%
    bad.push(l);
    let mut l = demo_limits();
    l.daily_cap = U128(1_000_001 * USD); // absurd cap
    bad.push(l);
    let mut l = demo_limits();
    l.price_floor = U128(u128::MAX);
    bad.push(l);
    let mut l = demo_limits();
    l.allowed_payees = vec![acc(AGENT)]; // the agent paying itself
    bad.push(l);
    let mut l = demo_limits();
    l.allowed_payees = (0..17).map(|i| acc(&format!("s{i}.testnet"))).collect();
    bad.push(l);
    for l in bad {
        let msg = panic_msg(|| {
            s.by_1y(BOB).set_limits(l.clone());
        });
        assert_eq!(msg, "BadLimits", "{l:?}");
    }
    // Negative or non-numeric amounts do not even parse.
    for args in [
        r#"{"price_floor":"-5","daily_cap":"50","allowed_payees":[],"split":{"owner_bps":10000,"treasury_bps":0}}"#,
        r#"{"price_floor":"0","daily_cap":"lots","allowed_payees":[],"split":{"owner_bps":10000,"treasury_bps":0}}"#,
        r#"{"price_floor":"0","daily_cap":"50","allowed_payees":[],"split":{"owner_bps":-1,"treasury_bps":10001}}"#,
    ] {
        assert!(serde_json::from_str::<LimitsArgs>(args).is_err(), "{args}");
    }
    assert_eq!(s.m().limits, agent_core::Limits::default());
}

#[test]
fn price_floor_refunds_small_jobs() {
    let mut s = business_world();
    let free = s.m().owner_free;
    assert_eq!(s.pay(PAYERS[3], 4 * USD), Some(4 * USD));
    assert!(refused("PolicyViolation:PriceFloor"));
    assert_eq!(s.m().owner_free, free);
}

#[test]
fn failed_supplier_payment_returns_to_treasury() {
    let mut s = business_world();
    s.runner(key(BOB_RUNNER))
        .pay(acc(SUMMARIZER), U128(8 * USD));
    assert_eq!(s.m().treasury, 2 * USD);
    s.callback(false)
        .on_transfer_out(OutKind::TreasuryPay, acc(SUMMARIZER), U128(8 * USD));
    assert_eq!(s.m().treasury, 10 * USD);
    assert!(s.m().solvent());
}

#[test]
fn hidden_key_cannot_spend_the_treasury() {
    let mut s = business_world();
    let r = s
        .runner(key(HIDDEN_KEY))
        .pay(acc(SUMMARIZER), U128(8 * USD));
    assert_eq!(r["frozen"], true);
    assert_eq!(s.m().treasury, 10 * USD);
    assert!(ft_transfers().iter().all(|(to, _)| to != SUMMARIZER));
}

// A24: the seller drains the treasury while listed.
fn listed_with_treasury() -> Sim {
    let mut s = translator_world();
    s.by_1y(ALICE).set_limits(demo_limits());
    for p in PAYERS.iter().take(5) {
        assert_eq!(s.pay(p, 60 * USD), Some(0));
    }
    let card = s
        .by(ALICE)
        .list_for_sale(U128(1_000 * USD), CODE_HASH.into(), None);
    assert_eq!(card["treasury_snapshot"], "60000000");
    s
}

#[test]
fn a24_no_spending_while_listed() {
    let mut s = listed_with_treasury();
    let before = s.m().clone();
    let msg = panic_msg(|| {
        s.runner(key(ALICE_RUNNER))
            .pay(acc(SUMMARIZER), U128(10 * USD));
    });
    assert_eq!(msg, "Listed: spending paused");
    assert_eq!(s.m(), &before);
    // Cancelling the listing resumes spending.
    s.by(ALICE).cancel_listing();
    let r = s
        .runner(key(ALICE_RUNNER))
        .pay(acc(SUMMARIZER), U128(10 * USD));
    assert_eq!(r["ok"], true);
}

#[test]
fn a24_buy_refused_if_treasury_below_listing_snapshot() {
    let mut s = listed_with_treasury();
    // Defense in depth: no method lowers the treasury while listed, but if any
    // path ever did, the buy is refused like a stale listing, full refund.
    s.inject_treasury_drain(10 * USD);
    assert_eq!(s.buy(BOB, 1_000 * USD, BOB_RUNNER), Some(1_000 * USD));
    assert!(refused("TreasuryChanged"));
    assert_eq!(s.m().owner, ALICE);
    // More money in the treasury than the snapshot is fine.
    s.c.m.treasury += 20 * USD;
    s.c.m.held += 20 * USD; // back above the snapshot
    assert_eq!(s.buy(BOB, 1_000 * USD, BOB_RUNNER), None);
}

#[test]
fn a24_payments_while_listed_still_fill_the_treasury() {
    let mut s = listed_with_treasury();
    assert_eq!(s.pay(PAYERS[5], 25 * USD), Some(0));
    assert_eq!(s.m().treasury, 65 * USD);
    assert_eq!(s.buy(BOB, 1_000 * USD, BOB_RUNNER), None);
}
