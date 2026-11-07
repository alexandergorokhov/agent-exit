//! Earn-out and handover scenarios on the pure machine (SCENARIOS.md ids).

use agent_core::*;

const T0: Ts = 1_767_225_600 * NANOS_PER_SEC; // 2026-01-01T00:00:00Z
const ALICE_PK: &str = "ed25519:alice-runner";
const BOB_PK: &str = "ed25519:bob-runner";

fn sold_agent() -> Machine {
    let mut m = Machine::new("translator", "alice");
    let deleted = m.set_runner_key("alice", ALICE_PK, T0).unwrap();
    m.runner_key_done(m.key_batch, &[], &deleted, ALICE_PK, true);
    m.list("alice", 1_000 * USD, Terms::default(), "hash", T0)
        .unwrap();
    let h = m.known_keys_hash();
    let plan = m
        .begin_buy("bob", 1_000 * USD, 1, &h, BOB_PK, true, T0)
        .unwrap();
    assert_eq!(plan.delete_keys, vec![ALICE_PK.to_string()]);
    match m.finish_buy(plan.sale_id, true, T0).unwrap() {
        BuyResult::Done { payout, unused, .. } => {
            assert_eq!(payout, 700 * USD);
            assert_eq!(unused, 0);
        }
        r => panic!("{r:?}"),
    }
    // The seller payout is in flight; it lands.
    m.transfer_done(TransferKind::SellerPayout, "alice", None, 700 * USD, true);
    m
}

fn end(m: &Machine) -> Ts {
    m.sale(1).expect("sale 1").window_end
}

#[test]
fn s5_core_handover_state() {
    let m = sold_agent();
    assert_eq!(m.owner, "bob");
    assert_eq!(m.owner_epoch, 2);
    assert_eq!(m.status, Status::Sold);
    assert!(m.known_keys.contains(BOB_PK) && !m.known_keys.contains(ALICE_PK));
    assert_eq!(m.escrow_left(), 300 * USD);
    assert!(m.solvent());
}

#[test]
fn s7_earnout_partial() {
    let mut m = sold_agent();
    for i in 0..4u64 {
        m.pay("p1", 25 * USD, false, T0 + (i + 1) * DAY_NS);
    }
    assert_eq!(m.claimable_of("alice"), 50 * USD);
    let s = m.finalize_earnout(1, end(&m)).unwrap();
    assert_eq!(s.released, 60 * USD);
    assert_eq!(s.returned, 240 * USD);
    assert_eq!(m.claimable_of("alice"), 60 * USD);
    assert_eq!(m.claimable_of("bob"), 240 * USD);
    assert!(m.solvent());
}

#[test]
fn s8_earnout_silence() {
    let mut m = sold_agent();
    assert_eq!(m.finalize_earnout(1, end(&m) - 1), Err(Error::WindowOpen));
    m.finalize_earnout(1, end(&m)).unwrap();
    assert_eq!(m.claimable_of("alice"), 60 * USD);
    assert_eq!(m.claimable_of("bob"), 240 * USD);
}

#[test]
fn s9_earnout_full() {
    let mut m = sold_agent();
    for i in 0..24u64 {
        m.pay(
            &format!("p{}", i % 6 + 1),
            25 * USD,
            false,
            T0 + i * 3_600 * NANOS_PER_SEC + 1,
        );
    }
    assert_eq!(m.sales[0].released, 300 * USD);
    m.finalize_earnout(1, end(&m)).unwrap();
    assert_eq!(m.claimable_of("alice"), 300 * USD);
    assert!(!(m.claimable_of("bob") > 0));
}

#[test]
fn s10_wash_after_sale() {
    let mut m = sold_agent();
    let r = m.pay("alice", 100 * USD, false, T0 + DAY_NS);
    match r {
        PayResult::Recorded {
            flags,
            counted,
            released,
            ..
        } => {
            assert!(flags.self_pay);
            assert!(!counted);
            assert!(released.is_empty());
        }
        r => panic!("{r:?}"),
    }
    assert!(!(m.claimable_of("alice") > 0));
}

#[test]
fn e21_finalize_twice_is_noop() {
    let mut m = sold_agent();
    let a = m.finalize_earnout(1, end(&m)).unwrap();
    let snapshot = m.clone();
    let b = m.finalize_earnout(1, end(&m) + DAY_NS).unwrap();
    assert_eq!(a, b);
    assert_eq!(m, snapshot);
}

#[test]
fn a23_core_hidden_key_freezes_and_cuts() {
    let mut m = sold_agent();
    m.pay("p1", 20 * USD, false, T0 + DAY_NS); // releases 10 to alice
    let r = m.runner_guard("translator", "ed25519:hidden").unwrap();
    assert_eq!(
        r,
        RunnerCheck::BackdoorProven {
            returned: vec![(1, "bob".into(), 290 * USD)]
        }
    );
    assert!(m.frozen);
    assert!(m.sale(1).unwrap().cut);
    assert!(m.sales.is_empty(), "the cut sale moved to the history");
    assert_eq!(m.claimable_of("bob"), 290 * USD);
    assert_eq!(m.runner_guard("translator", BOB_PK), Err(Error::Frozen));
    // The owner can still take revenue out (I10).
    assert_eq!(m.withdraw("bob", 20 * USD, "bob"), Ok(20 * USD));
    assert!(m.solvent());
}

#[test]
fn s15_resale_first_earnout_keeps_releasing() {
    let mut m = sold_agent();
    m.list("bob", 2_000 * USD, Terms::default(), "hash", T0 + DAY_NS)
        .unwrap();
    let h = m.known_keys_hash();
    let plan = m
        .begin_buy(
            "carol",
            2_000 * USD,
            m.listing_version,
            &h,
            "ed25519:carol",
            true,
            T0 + DAY_NS,
        )
        .unwrap();
    assert_eq!(plan.delete_keys, vec![BOB_PK.to_string()]);
    m.finish_buy(plan.sale_id, true, T0 + DAY_NS).unwrap();
    assert_eq!(m.owner, "carol");
    // alice and bob are both past owners: their payments are `self`.
    assert!(m.flags_for("alice").self_pay && m.flags_for("bob").self_pay);
    m.pay("p1", 10 * USD, false, T0 + 2 * DAY_NS);
    // Oldest earn-out first: alice's.
    assert_eq!(m.sales[0].released, 5 * USD);
    assert_eq!(m.sales[1].released, 0);
}

// ----- PR1-PR5: pay from revenue (the whole price held in the earn-out) -----

fn pay_from_revenue_terms() -> Terms {
    Terms {
        earnout_bps: MAX_EARNOUT_BPS,
        floor_bps: 0,
        ..Terms::default()
    }
}

/// alice lists at 1,000 with the whole price held; bob buys. The protocol
/// fee account is on, as in the contract.
fn sold_from_revenue() -> Machine {
    let mut m = Machine::new("translator", "alice");
    m.fee_to = "fees".into();
    let deleted = m.set_runner_key("alice", ALICE_PK, T0).unwrap();
    m.runner_key_done(m.key_batch, &[], &deleted, ALICE_PK, true);
    m.list("alice", 1_000 * USD, pay_from_revenue_terms(), "hash", T0)
        .unwrap();
    let h = m.known_keys_hash();
    let plan = m
        .begin_buy("bob", 1_000 * USD, 1, &h, BOB_PK, true, T0)
        .unwrap();
    match m.finish_buy(plan.sale_id, true, T0).unwrap() {
        BuyResult::Done {
            payout, fee, sale, ..
        } => {
            assert_eq!(payout, 0, "nothing to the seller at the sale");
            assert_eq!(fee, 0, "the fee comes out of the upfront part, which is 0");
            assert_eq!(sale.escrow, 1_000 * USD);
        }
        r => panic!("{r:?}"),
    }
    m
}

#[test]
fn pr1_pay_from_revenue_sale_pays_the_seller_nothing_now() {
    let m = sold_from_revenue();
    assert_eq!(m.owner, "bob");
    assert!(m.known_keys.contains(BOB_PK) && !m.known_keys.contains(ALICE_PK));
    assert_eq!(m.escrow_left(), 1_000 * USD);
    assert_eq!(m.claimable_of("alice"), 0);
    assert_eq!(m.claimable_of("fees"), 0);
    assert!(m.solvent());
}

#[test]
fn pr2_the_fee_never_exceeds_the_upfront_part() {
    // 99% held: upfront 10.00, fee 1% of 1,000 = 10.00, seller gets 0 now.
    let mut m = Machine::new("translator", "alice");
    m.fee_to = "fees".into();
    let deleted = m.set_runner_key("alice", ALICE_PK, T0).unwrap();
    m.runner_key_done(m.key_batch, &[], &deleted, ALICE_PK, true);
    let t = Terms {
        earnout_bps: 9_950,
        ..pay_from_revenue_terms()
    };
    m.list("alice", 1_000 * USD, t, "hash", T0).unwrap();
    let h = m.known_keys_hash();
    let plan = m
        .begin_buy("bob", 1_000 * USD, 1, &h, BOB_PK, true, T0)
        .unwrap();
    match m.finish_buy(plan.sale_id, true, T0).unwrap() {
        BuyResult::Done { payout, fee, .. } => {
            assert_eq!(fee, 5 * USD, "capped at the 0.5% upfront part");
            assert_eq!(payout, 0);
            assert_eq!(m.claimable_of("fees"), fee, "credited = taken");
        }
        r => panic!("{r:?}"),
    }
    assert!(m.solvent());
}

#[test]
fn pr3_the_seller_is_paid_only_by_customer_revenue() {
    let mut m = sold_from_revenue();
    // 40 customer payments of 50.00 over 20 days: half of each is released.
    for i in 0..40u64 {
        m.pay(
            &format!("c{}", i % 8),
            50 * USD,
            false,
            T0 + (i + 1) * DAY_NS / 2,
        );
    }
    assert_eq!(
        m.claimable_of("alice"),
        1_000 * USD,
        "paid in full, from revenue"
    );
    assert_eq!(m.escrow_left(), 0);
    assert!(m.solvent());
}

#[test]
fn pr4_no_revenue_means_the_buyer_gets_the_price_back() {
    let mut m = sold_from_revenue();
    m.pay("c1", 100 * USD, false, T0 + DAY_NS);
    let s = m.finalize_earnout(1, end(&m)).unwrap();
    assert_eq!(s.released, 50 * USD);
    assert_eq!(s.returned, 950 * USD);
    assert_eq!(m.claimable_of("alice"), 50 * USD);
    assert_eq!(m.claimable_of("bob"), 950 * USD);
    assert!(m.solvent());
}

#[test]
fn pr5_faking_revenue_after_the_sale_costs_twice_what_it_releases() {
    let t = pay_from_revenue_terms();
    assert_eq!(t.cost_to_fake_release(100 * USD), 200 * USD);
    assert!(t.pay_from_revenue() && !Terms::default().pay_from_revenue());
    // A fresh wallet the seller controls looks like a customer: it releases
    // at most half of what it pays, so the seller is 100.00 down.
    let mut m = sold_from_revenue();
    m.pay("fresh-wallet", 200 * USD, false, T0 + DAY_NS);
    assert_eq!(m.claimable_of("alice"), 100 * USD);
    // More than 100% held is refused.
    let bad = Terms {
        earnout_bps: MAX_EARNOUT_BPS + 1,
        ..t
    };
    assert_eq!(bad.check(&TermsMin::PROD), Err(Error::BadTerms));
}
