//! Security review 4: the reviewer's scratch tests, with the asserts
//! inverted (they asserted the defect). Scenario ids A50 and up.

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

/// NEAR sent to `who` by the receipts of the last call.
fn refunds_to(who: &str) -> Vec<u128> {
    near_sdk::test_utils::get_created_receipts()
        .into_iter()
        .filter(|r| r.receiver_id.as_str() == who)
        .flat_map(|r| r.actions)
        .filter_map(|a| match a {
            near_sdk::mock::MockAction::Transfer { deposit, .. } => Some(deposit.as_yoctonear()),
            _ => None,
        })
        .collect()
}

fn as_mallory_for(s: &mut Sim, who: &str) {
    let mut c = s.call(MALLORY);
    c.deposit = PAYER_ROW_DEPOSIT;
    s.env(&c);
    s.c.storage_deposit(Some(acc(who)), None);
}

fn unregister(s: &mut Sim, who: &str) -> bool {
    let mut c = s.call(who);
    c.deposit = NearToken::from_yoctonear(1);
    s.env(&c);
    s.c.storage_unregister(None)
}

fn reclaim(s: &mut Sim, who: &str) -> Value {
    s.env(&s.call(CAROL));
    s.c.reclaim_payer_row(acc(who))
}

// ------------------------------------------------------------- M1 (A50)

/// A50 (M1, R4-1 inverted): an outsider registering 1,000 third-party rows
/// does not stop a real customer getting a row and being counted: there is
/// no cap on rows, each paid for by its depositor.
#[test]
fn a50_outsider_rows_do_not_fill_the_payer_table() {
    let mut s = translator_world();
    let start = s.m().meter.tracked_payers;
    let empty0 = s.m().meter.empty_payers;
    for i in 0..1_000 {
        as_mallory_for(&mut s, &format!("g{i}.testnet"));
    }
    assert_eq!(s.m().meter.tracked_payers, start + 1_000);
    assert_eq!(s.m().meter.empty_payers, empty0 + 1_000);
    s.register("real-customer.testnet");
    assert!(s.c.get_payer(acc("real-customer.testnet")).is_some());
    let before = s.m().meter.counted_payers;
    s.pay("real-customer.testnet", 50 * USD);
    assert_eq!(s.m().meter.counted_payers, before + 1, "counted as a payer");
    assert_eq!(
        s.m().meter.empty_payers,
        empty0 + 1_000,
        "its row is not empty"
    );
    let st = s.by(MALLORY).get_status();
    assert_eq!(st["empty_payer_rows"], empty0 + 1_000);
    assert_eq!(st["tracked_payers"], start + 1_001);
}

/// A50: NEP-145 `storage_unregister` removes the caller's own row if it
/// never had a payment, refunding the deposit; a row with payments stays.
#[test]
fn a50_storage_unregister_removes_only_empty_rows() {
    let mut s = translator_world();
    as_mallory_for(&mut s, "g0.testnet");
    let tracked = s.m().meter.tracked_payers;
    let empty = s.m().meter.empty_payers;
    assert!(unregister(&mut s, "g0.testnet"));
    assert_eq!(
        refunds_to("g0.testnet"),
        vec![PAYER_ROW_DEPOSIT.as_yoctonear() + 1],
        "the deposit and the 1 yocto go back to the account"
    );
    assert!(s.c.get_payer(acc("g0.testnet")).is_none());
    assert!(s.c.storage_balance_of(acc("g0.testnet")).is_none());
    assert_eq!(s.m().meter.tracked_payers, tracked - 1);
    assert_eq!(s.m().meter.empty_payers, empty - 1);
    // No row: false, nothing sent back but the 1 yocto stays with the call.
    assert!(!unregister(&mut s, "g0.testnet"));
    // A row with a payment is part of the earnings record.
    s.register("payer-a.testnet");
    s.pay("payer-a.testnet", 5 * USD);
    assert_eq!(
        panic_text(|| {
            unregister(&mut s, "payer-a.testnet");
        }),
        "PayerHasPayments"
    );
    assert!(s.c.get_payer(acc("payer-a.testnet")).is_some());
    // Without the 1 yocto (a function-call key) it is refused.
    let mut c = s.call("payer-a.testnet");
    c.deposit = NearToken::from_yoctonear(0);
    s.env(&c);
    assert!(!panic_text(|| {
        s.c.storage_unregister(None);
    })
    .is_empty());
}

/// A50: anyone may reclaim a row that had no payment for
/// `EMPTY_ROW_TTL_NS`; the deposit goes back to whoever paid it.
#[test]
fn a50_empty_rows_are_reclaimable_after_the_ttl() {
    let mut s = translator_world();
    as_mallory_for(&mut s, "g1.testnet");
    as_mallory_for(&mut s, "g2.testnet");
    s.pay("g2.testnet", 5 * USD);
    assert_eq!(
        panic_text(|| {
            reclaim(&mut s, "g1.testnet");
        }),
        "RowTooYoung"
    );
    s.advance(EMPTY_ROW_TTL_NS);
    let tracked = s.m().meter.tracked_payers;
    let ev = reclaim(&mut s, "g1.testnet");
    assert_eq!(ev["refund_to"], MALLORY);
    assert_eq!(
        refunds_to(MALLORY),
        vec![PAYER_ROW_DEPOSIT.as_yoctonear()],
        "the depositor gets the deposit back"
    );
    assert_eq!(s.m().meter.tracked_payers, tracked - 1);
    assert!(s.c.get_payer(acc("g1.testnet")).is_none());
    assert_eq!(
        panic_text(|| {
            reclaim(&mut s, "g1.testnet");
        }),
        "NotRegistered"
    );
    // A row with a payment is never reclaimed.
    assert_eq!(
        panic_text(|| {
            reclaim(&mut s, "g2.testnet");
        }),
        "PayerHasPayments"
    );
    // A reclaimed account can register again.
    s.register("g1.testnet");
    assert!(s.c.get_payer(acc("g1.testnet")).is_some());
}

// ------------------------------------------------------------- L1 (A52)

fn base_limits() -> LimitsArgs {
    let mut l = demo_limits();
    l.base = Some(crate::BaseLimitsArgs {
        daily_cap: U128(5_000_000),
        payees: vec!["0x00000000000000000000000000000000000000a1".into()],
    });
    l
}

/// A52 (L1, R4-3 inverted): `set_limits` without `base` keeps the Base
/// limits; an explicit empty value clears them.
#[test]
fn a52_set_limits_without_base_keeps_base_limits() {
    let mut s = sold_translator();
    s.by_1y(BOB).set_limits(base_limits());
    assert_eq!(s.m().base.limits.payees.len(), 1);
    let mut l = demo_limits();
    l.price_floor = U128(7 * USD);
    s.by_1y(BOB).set_limits(l); // base: None
    assert_eq!(s.m().base.limits.payees.len(), 1);
    assert_eq!(s.m().base.limits.daily_cap, 5_000_000);
    assert_eq!(s.m().limits.price_floor, 7 * USD, "the NEAR limits changed");
    let mut clear = demo_limits();
    clear.base = Some(crate::BaseLimitsArgs {
        daily_cap: U128(0),
        payees: vec![],
    });
    s.by_1y(BOB).set_limits(clear);
    assert!(s.m().base.limits.payees.is_empty());
    assert_eq!(s.m().base.limits.daily_cap, 0);
}

// ------------------------------------------------------------- L2 (A53)

/// A53 (L2, R4-4 inverted): 300 deed moves, each with a payout to a new
/// destination, leave the state blob the same size: payout destinations
/// are counters plus a pruned-prefix watermark, not a map per epoch.
#[test]
fn a53_blob_is_flat_in_owner_epochs() {
    let mut s = sold_translator();
    let b0 = s.c.state_bytes();
    let mut owner = BOB.to_string();
    for i in 0..300 {
        let next = format!("alt{i}.testnet");
        s.pay(&format!("cust{i}.testnet"), USD);
        let free = s.m().owner_free;
        if free > 0 {
            s.by_1y(&owner)
                .withdraw(U128(free), Some(acc(&format!("dest{i}.testnet"))))
                .detach();
        }
        s.by_1y(&owner)
            .nft_transfer(acc(&next), DEED_ID.into(), None, None);
        let keys: Vec<String> = s.m().retiring.iter().cloned().collect();
        s.callback(true).on_keys_deleted(keys);
        owner = next;
    }
    let b1 = s.c.state_bytes();
    assert!(s.m().paid_to_total() >= 300, "the destinations are tracked");
    assert_eq!(s.m().paid_to_epoch("dest0.testnet"), Some(2));
    assert!(b1 <= b0 + 16, "blob {b0} -> {b1}");
}

/// A53: pruning is still exact: once no sale is open, every destination up
/// to the newest finalized sale's era is dropped, and later ones stay.
#[test]
fn a53_prune_drops_the_sold_prefix_only() {
    let mut s = sold_translator();
    // Bob (epoch 2) pays a destination; then moves the deed to himself.
    s.pay("cust.testnet", 10 * USD);
    let free = s.m().owner_free;
    s.by_1y(BOB)
        .withdraw(U128(free / 2), Some(acc("d-bob.testnet")))
        .detach();
    assert_eq!(s.m().paid_to_this_owner(), 1);
    s.by_1y(BOB)
        .nft_transfer(acc("bob2.testnet"), DEED_ID.into(), None, None);
    let keys: Vec<String> = s.m().retiring.iter().cloned().collect();
    s.callback(true).on_keys_deleted(keys);
    assert_eq!(s.m().paid_to_this_owner(), 0, "new epoch");
    s.pay("cust.testnet", 10 * USD);
    let free = s.m().owner_free;
    s.by_1y("bob2.testnet")
        .withdraw(U128(free), Some(acc("d-bob2.testnet")))
        .detach();
    let total = s.m().paid_to_total();
    // Sale 1 (Alice's era, epoch 1) finalizes: only entries up to epoch 1
    // die; Bob's (epochs 2 and 3) stay.
    s.advance(31 * DAY);
    s.env(&s.call(MALLORY));
    let _ = s.c.finalize_earnout(1);
    assert_eq!(s.m().paid_to_dead_upto, 1);
    assert_eq!(s.m().paid_to_epoch("d-bob.testnet"), Some(2));
    assert_eq!(s.m().paid_to_epoch("d-bob2.testnet"), Some(3));
    assert!(s.m().paid_to_total() <= total);
    assert_eq!(
        s.m().paid_to_total(),
        s.m().paid_to_ever - s.m().paid_to_dead_count
    );
}

// ------------------------------------------------------------- L3 (A54)

const HOUR: u64 = 3_600 * agent_core::NANOS_PER_SEC;

fn verify_note() -> String {
    events("payment")[0]["verify"].as_str().unwrap().to_string()
}

fn fill_shared_window(s: &mut Sim) {
    let now_s = s.now / agent_core::NANOS_PER_SEC;
    s.env(&s.call(AGENT));
    for i in 0..verified::MAX_SIGS {
        let mut id = [0xffu8; 32];
        id[..4].copy_from_slice(&i.to_le_bytes());
        s.c.used_sigs.insert(id, now_s + 60, now_s);
    }
}

/// A54 (L3 inverted): outsiders filling the shared signature window do not
/// deny a registered payer's badge: it has its own slots, paid for by its
/// row deposit. An unregistered payer still gets `SigWindowFull`.
#[test]
fn a54_registered_payers_keep_their_badge_when_the_window_is_full() {
    let mut s = Sim::new(AGENT, ALICE);
    s.now = T0 + HOUR;
    fill_shared_window(&mut s);
    assert_eq!(s.c.used_sigs.len(), verified::MAX_SIGS);
    // Not registered: the shared window is full.
    s.ft(PAYERS[0], 12 * USD, &job_msg(&job(0)), 100);
    assert_eq!(verify_note(), "SigWindowFull");
    // Registered: its own slot.
    s.register(PAYERS[0]);
    s.ft(PAYERS[0], 12 * USD, &job_msg(&job(0)), 100);
    assert!(verify_note().starts_with("ok:"), "{}", verify_note());
    assert_eq!(s.c.used_sigs.len(), verified::MAX_SIGS, "shared not used");
    // The id is used, whichever lane holds it.
    s.ft(PAYERS[0], 12 * USD, &job_msg(&job(0)), 100);
    assert_eq!(verify_note(), "SignatureUsed");
}

/// A54: a payer's own slots are bounded (`PAYER_SIGS`), reused once
/// expired, and fit in what the row deposit paid for.
#[test]
fn a54_payer_slots_are_bounded_and_paid_for() {
    let mut s = Sim::new(AGENT, ALICE);
    s.env(&s.call(AGENT));
    let now_s = s.now / agent_core::NANOS_PER_SEC;
    let payer = format!("{}.testnet", "p".repeat(56));
    assert_eq!(payer.len(), 64);
    s.c.used_sigs.flush();
    let before = near_sdk::env::storage_usage();
    for i in 0..verified::PAYER_SIGS {
        assert!(s.c.used_sigs.has_room(Some(&payer), now_s));
        s.c.used_sigs
            .insert_for(Some(&payer), [i; 32], now_s + 60, now_s);
    }
    s.c.used_sigs.flush();
    let grew = (near_sdk::env::storage_usage() - before) as u128;
    assert!(
        grew <= verified::PAYER_SIGS as u128 * PAYER_SIG_SLOT_BYTES,
        "{grew} bytes"
    );
    assert_eq!(s.c.used_sigs.len(), 0, "all in the payer's own slots");
    // Own slots full: the next goes to the shared window.
    assert!(s.c.used_sigs.has_room(Some(&payer), now_s));
    s.c.used_sigs
        .insert_for(Some(&payer), [0xaa; 32], now_s + 60, now_s);
    assert_eq!(s.c.used_sigs.len(), 1);
    // Expired slots are reused and their old ids dropped.
    let later = now_s + 120;
    s.c.used_sigs
        .insert_for(Some(&payer), [0xbb; 32], later + 60, later);
    assert!(!s.c.used_sigs.contains(&[0u8; 32]));
    assert!(s.c.used_sigs.contains(&[0xbb; 32]));
    assert_eq!(s.c.used_sigs.len(), 1);
}

/// The row price the sandbox test and the testnet seed attach as a literal
/// (tests/sandbox/tests/b1.rs, tools/testnet/src/steps.rs).
#[test]
fn a54_row_price_matches_the_tools() {
    assert_eq!(PAYER_ROW_DEPOSIT, NearToken::from_millinear(24));
}

/// A50: the owner cannot withdraw the NEAR held for payer rows, so an empty
/// row's refund and a payer's signature slots stay covered.
#[test]
fn a50_owner_cannot_withdraw_payer_deposits() {
    let mut s = business_world();
    let before = s.by(MALLORY).near_withdrawable().as_yoctonear();
    let held0 = s.c.payer_deposits;
    for i in 0..10 {
        as_mallory_for(&mut s, &format!("g{i}.testnet"));
    }
    assert_eq!(
        s.c.payer_deposits,
        held0 + 10 * PAYER_ROW_DEPOSIT.as_yoctonear()
    );
    // The deposits arrive in the balance; none of it becomes withdrawable.
    s.balance = s
        .balance
        .saturating_add(PAYER_ROW_DEPOSIT.saturating_mul(10));
    let after = s.by(MALLORY).near_withdrawable().as_yoctonear();
    assert!(after <= before, "{before} -> {after}");
    unregister(&mut s, "g0.testnet");
    assert_eq!(
        s.c.payer_deposits,
        held0 + 9 * PAYER_ROW_DEPOSIT.as_yoctonear()
    );
}

// ------------------------------------------------- review 5 (C27, C30, C32)

/// C27: the signer list is capped, so a verified payment's ecrecover loop
/// stays inside the gas limit; re-adding a listed address is still fine.
#[test]
fn c27_signer_list_is_capped() {
    let mut s = translator_world();
    let have = s.by(BOB).get_signers().len();
    for i in have..crate::MAX_SIGNERS {
        s.by_1y(ADMIN).add_signer(format!("0x{:040x}", i + 1));
    }
    assert_eq!(s.by(BOB).get_signers().len(), crate::MAX_SIGNERS);
    s.by_1y(ADMIN).add_signer(test_signer());
    let e = panic_text(|| s.by_1y(ADMIN).add_signer(format!("0x{:040x}", 999)));
    assert_eq!(e, "TooManySigners");
    // A signed payment still verifies with the full list.
    s.register(PAYERS[0]);
    assert_eq!(s.ft(PAYERS[0], 12 * USD, &job_msg(&job(0)), 100), Some(0));
}

/// C30: the account that created the agent (its parent) pays as self.
#[test]
fn c30_the_agents_ancestor_pays_as_self() {
    let mut s = Sim::new("bot.alice.testnet", CAROL);
    for (p, counted) in [
        (ALICE, false),
        (BOB, true),
        ("lice.testnet", true),
        ("testnet", false),
    ] {
        s.register(p);
        assert_eq!(s.pay(p, 20 * USD), Some(0));
        let ev = &events("payment")[0];
        assert_eq!(ev["counted"], counted, "{p}: {ev}");
    }
    assert_eq!(events("payment").len(), 1);
}

/// C32: a frozen agent (backdoor proven) refunds new payments in full.
#[test]
fn c32_pay_on_a_frozen_agent_is_refunded() {
    let mut s = translator_world();
    s.c.m.frozen = true;
    let gross = s.m().earnings(s.now).gross;
    assert_eq!(s.pay(PAYERS[0], 12 * USD), Some(12 * USD));
    assert_eq!(s.m().earnings(s.now).gross, gross);
}
