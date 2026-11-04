//! Shares of future earnings: rules SH1-SH11 and threats T1-T17
//! (docs/PLAN-shares.md), each as a test on the pure machine.

use agent_core::*;

const T0: Ts = 1_767_225_600 * NANOS_PER_SEC; // 2026-01-01T00:00:00Z
const YEAR: Ts = 365 * DAY_NS;
const ALICE_PK: &str = "ed25519:alice-runner";
const BOB_PK: &str = "ed25519:bob-runner";

fn agent() -> Machine {
    let mut m = Machine::new("translator", "alice");
    let deleted = m.set_runner_key("alice", ALICE_PK, T0).unwrap();
    m.runner_key_done(m.key_batch, &[], &deleted, ALICE_PK, true);
    m
}

/// Conservation: every token the agent holds is in exactly one bucket.
fn conserved(m: &Machine) {
    assert_eq!(
        m.held,
        m.escrow_left() + m.claimable_total() + m.owner_free + m.treasury + m.share_escrow,
        "conservation"
    );
    assert!(m.solvent());
}

/// alice offers 20% for a year at 500; dave buys all of it.
fn with_dave() -> Machine {
    let mut m = agent();
    let id = m
        .offer_share("alice", 2_000, 500 * USD, T0 + YEAR, T0)
        .unwrap();
    let t = m.offer(id).unwrap().terms();
    assert_eq!(
        m.buy_share("dave", 500 * USD, t, 2_000, T0).unwrap(),
        500 * USD
    );
    conserved(&m);
    m
}

#[test]
fn sh2_t1_investors_paid_inside_pay() {
    let mut m = with_dave();
    // SH11: alice got half now, half waits in escrow.
    assert_eq!(m.owner_free, 250 * USD);
    assert_eq!(m.share_escrow, 250 * USD);
    match m.pay("p1", 100 * USD, false, T0 + DAY_NS) {
        PayResult::Recorded {
            to_owner,
            to_treasury,
            to_investors,
            share_released,
            ..
        } => {
            assert_eq!(to_investors, 20 * USD);
            assert_eq!(to_owner + to_treasury + to_investors, 100 * USD);
            assert_eq!(share_released, vec![("alice".to_string(), 20 * USD)]);
        }
        r => panic!("{r:?}"),
    }
    assert_eq!(m.claimable_of("dave"), 20 * USD);
    assert_eq!(m.share_escrow, 230 * USD);
    // T1: alice can withdraw only her own revenue, never dave's 20.
    let free = m.owner_free;
    assert_eq!(
        m.withdraw("alice", free + 1, "alice"),
        Err(Error::InsufficientFunds)
    );
    m.withdraw("alice", free, "alice").unwrap();
    assert_eq!(m.claimable_of("dave"), 20 * USD);
    conserved(&m);
}

#[test]
fn t3_investors_take_from_gross_before_treasury() {
    let mut m = with_dave();
    let mut l = m.limits.clone();
    l.treasury_bps = 5_000;
    l.owner_bps = 5_000;
    m.set_limits("alice", l).unwrap();
    m.pay("p1", 100 * USD, false, T0 + DAY_NS);
    assert_eq!(m.claimable_of("dave"), 20 * USD);
    conserved(&m);
}

#[test]
fn sh8_self_payment_still_pays_investors() {
    let mut m = with_dave();
    for payer in ["alice", "x.alice", "translator"] {
        match m.pay(payer, 50 * USD, false, T0 + DAY_NS) {
            PayResult::Recorded {
                counted,
                to_investors,
                ..
            } => {
                assert!(!counted, "{payer} is flagged");
                assert_eq!(to_investors, 10 * USD);
            }
            r => panic!("{r:?}"),
        }
    }
    assert_eq!(m.claimable_of("dave"), 30 * USD);
    // Review M2: flagged payments release no escrow, so a self-payment
    // cannot unlock the held half of the price.
    assert_eq!(m.share_escrow, 250 * USD);
    conserved(&m);
}

#[test]
fn sh11_t2_escrow_refunded_when_owner_walks_away() {
    let mut m = with_dave();
    // Customers go elsewhere: only 50 paid in a year.
    m.pay("p1", 50 * USD, false, T0 + DAY_NS);
    assert_eq!(m.close_share(1, T0 + YEAR - 1), Err(Error::WindowOpen));
    let refunds = m.close_share(1, T0 + YEAR).unwrap();
    assert_eq!(refunds, vec![("dave".to_string(), 240 * USD)]);
    // dave: 10 earned + 240 back = 250 of 500; alice kept 250 + 10 released.
    assert_eq!(m.claimable_of("dave"), 250 * USD);
    assert!(m.offers.is_empty());
    // SH4: after closing, payments go to the owner again.
    match m.pay("p1", 10 * USD, false, T0 + YEAR + 1) {
        PayResult::Recorded { to_investors, .. } => assert_eq!(to_investors, 0),
        r => panic!("{r:?}"),
    }
    conserved(&m);
}

#[test]
fn sh11_escrow_fully_released_when_agent_earns() {
    let mut m = with_dave();
    for d in 1..=30u64 {
        m.pay("p1", 50 * USD, false, T0 + d * DAY_NS);
    }
    // dave earned 300 > 250 escrow: all of it went to alice.
    assert_eq!(m.claimable_of("dave"), 300 * USD);
    assert_eq!(m.share_escrow, 0);
    assert_eq!(m.close_share(1, T0 + YEAR).unwrap(), vec![]);
    conserved(&m);
}

#[test]
fn sh4_ended_slice_pays_nothing() {
    let mut m = with_dave();
    match m.pay("p1", 100 * USD, false, T0 + YEAR) {
        PayResult::Recorded { to_investors, .. } => assert_eq!(to_investors, 0),
        r => panic!("{r:?}"),
    }
}

#[test]
fn sh1_total_capped_at_49_percent() {
    let mut m = agent();
    m.offer_share("alice", 3_000, 100 * USD, T0 + YEAR, T0)
        .unwrap();
    assert_eq!(
        m.offer_share("alice", 2_000, 100 * USD, T0 + YEAR, T0),
        Err(Error::SharesTooHigh)
    );
    m.offer_share("alice", 1_900, 100 * USD, T0 + YEAR, T0)
        .unwrap();
    assert_eq!(
        m.offer_share("alice", 100, 100 * USD, T0 + YEAR, T0),
        Err(Error::TooManyOffers)
    );
}

#[test]
fn offer_bounds_and_owner_only() {
    let mut m = agent();
    let ok_end = T0 + YEAR;
    assert_eq!(
        m.offer_share("bob", 1_000, USD, ok_end, T0),
        Err(Error::NotOwner)
    );
    assert_eq!(
        m.offer_share("alice", 99, USD, ok_end, T0),
        Err(Error::BadShareTerms)
    );
    assert_eq!(
        m.offer_share("alice", 1_000, 0, ok_end, T0),
        Err(Error::BadShareTerms)
    );
    assert_eq!(
        m.offer_share("alice", 1_000, USD, T0 + 1, T0),
        Err(Error::BadShareTerms)
    );
    assert_eq!(
        m.offer_share("alice", 1_000, USD, T0 + 3 * YEAR, T0),
        Err(Error::BadShareTerms)
    );
}

#[test]
fn sh5_t10_holder_cap_and_min_part() {
    let mut m = agent();
    let id = m
        .offer_share("alice", 4_000, 1_000 * USD, T0 + YEAR, T0)
        .unwrap();
    let t = m.offer(id).unwrap().terms();
    // T10: a part below a tenth of the slice is refused.
    assert_eq!(
        m.buy_share("i0", 1_000 * USD, t, 399, T0),
        Err(Error::ShareTooSmall)
    );
    // Ten minimum parts sell the whole slice: blocking costs the full price.
    for i in 0..10 {
        m.buy_share(&format!("i{i}"), 100 * USD, t, 400, T0)
            .unwrap();
    }
    assert_eq!(
        m.buy_share("late", 100 * USD, t, 400, T0),
        Err(Error::OfferClosed)
    );
    conserved(&m);
    // SH5 over two slices: at most 16 investors.
    let id2 = m
        .offer_share("alice", 900, 900 * USD, T0 + YEAR, T0)
        .unwrap();
    let t2 = m.offer(id2).unwrap().terms();
    for i in 10..16 {
        m.buy_share(&format!("i{i}"), 90 * USD, t2, 90, T0).unwrap();
    }
    assert_eq!(
        m.buy_share("i16", 90 * USD, t2, 90, T0),
        Err(Error::TooManyHolders)
    );
    // An existing holder may add to its part.
    m.buy_share("i10", 90 * USD, t2, 90, T0).unwrap();
    conserved(&m);
}

#[test]
fn t6_changed_terms_refused_and_overpay_returned() {
    let mut m = agent();
    let id = m
        .offer_share("alice", 2_000, 500 * USD, T0 + YEAR, T0)
        .unwrap();
    let mut t = m.offer(id).unwrap().terms();
    t.price = 400 * USD;
    assert_eq!(
        m.buy_share("dave", 500 * USD, t, 2_000, T0),
        Err(Error::ShareChanged)
    );
    t.price = 500 * USD;
    assert_eq!(
        m.buy_share("dave", 499 * USD, t, 2_000, T0),
        Err(Error::Underpaid)
    );
    // Half the slice costs 250; the contract keeps only the cost.
    assert_eq!(
        m.buy_share("dave", 600 * USD, t, 1_000, T0).unwrap(),
        250 * USD
    );
    conserved(&m);
}

#[test]
fn t7_terms_fixed_once_sold() {
    let mut m = agent();
    let id = m
        .offer_share("alice", 1_000, 100 * USD, T0 + YEAR, T0)
        .unwrap();
    let t = m.offer(id).unwrap().terms();
    m.buy_share("dave", 10 * USD, t, 100, T0).unwrap();
    assert_eq!(m.cancel_offer("alice", id), Err(Error::SharesOpen));
    let id2 = m
        .offer_share("alice", 1_000, 100 * USD, T0 + YEAR, T0)
        .unwrap();
    assert_eq!(m.cancel_offer("bob", id2), Err(Error::NotOwner));
    m.cancel_offer("alice", id2).unwrap();
}

#[test]
fn t4_price_floor_frozen_while_held() {
    let mut m = with_dave();
    let mut l = m.limits.clone();
    l.price_floor = 1_000 * USD;
    assert_eq!(m.set_limits("alice", l.clone()), Err(Error::SharesOpen));
    // Lowering it or changing the rest is fine.
    l.price_floor = 0;
    m.set_limits("alice", l).unwrap();
}

#[test]
fn self_and_subaccount_cannot_invest() {
    let mut m = agent();
    let id = m
        .offer_share("alice", 1_000, 100 * USD, T0 + YEAR, T0)
        .unwrap();
    let t = m.offer(id).unwrap().terms();
    for who in ["alice", "x.alice", "translator"] {
        assert_eq!(
            m.buy_share(who, 100 * USD, t, 1_000, T0),
            Err(Error::SelfBuy)
        );
    }
}

#[test]
fn sh6_no_offer_or_buy_while_listed() {
    let mut m = agent();
    let id = m
        .offer_share("alice", 1_000, 100 * USD, T0 + YEAR, T0)
        .unwrap();
    let t = m.offer(id).unwrap().terms();
    m.list("alice", 1_000 * USD, Terms::default(), "hash", T0)
        .unwrap();
    assert_eq!(
        m.offer_share("alice", 1_000, 100 * USD, T0 + YEAR, T0),
        Err(Error::AlreadyListed)
    );
    assert_eq!(
        m.buy_share("dave", 100 * USD, t, 1_000, T0),
        Err(Error::SaleInProgress)
    );
}

#[test]
fn t9_sale_keeps_shares_and_escrow_goes_to_old_seller() {
    let mut m = with_dave();
    m.list("alice", 1_000 * USD, Terms::default(), "hash", T0)
        .unwrap();
    let h = m.known_keys_hash();
    let v = m.listing_version;
    let plan = m
        .begin_buy("bob", 1_000 * USD, v, &h, BOB_PK, true, T0)
        .unwrap();
    m.finish_buy(plan.sale_id, true, T0).unwrap();
    assert_eq!(m.owner, "bob");
    assert_eq!(m.shares_bps(), 2_000);
    let alice_before = m.claimable_of("alice");
    match m.pay("p1", 100 * USD, false, T0 + DAY_NS) {
        PayResult::Recorded {
            to_investors,
            share_released,
            ..
        } => {
            assert_eq!(to_investors, 20 * USD);
            assert_eq!(share_released, vec![("alice".to_string(), 20 * USD)]);
        }
        r => panic!("{r:?}"),
    }
    assert_eq!(m.claimable_of("dave"), 20 * USD);
    // alice's escrow release lands in her claimable (she no longer owns),
    // plus her earn-out release.
    assert!(m.claimable_of("alice") >= alice_before + 20 * USD);
    // bob cannot cancel or alter alice's sold slice.
    assert_eq!(m.cancel_offer("bob", 1), Err(Error::SharesOpen));
}

#[test]
fn t12_claim_pays_only_the_caller() {
    let mut m = with_dave();
    m.pay("p1", 100 * USD, false, T0 + DAY_NS);
    assert_eq!(m.claim("mallory"), Err(Error::NothingToClaim));
    assert_eq!(m.claim("dave").unwrap(), 20 * USD);
    conserved(&m);
}

#[test]
fn t13_rounding_stays_with_owner() {
    let mut m = agent();
    let id = m.offer_share("alice", 333, 7 * USD, T0 + YEAR, T0).unwrap();
    let t = m.offer(id).unwrap().terms();
    m.buy_share("dave", 7 * USD, t, 333, T0).unwrap();
    for i in 0..50u64 {
        if let PayResult::Recorded {
            to_owner,
            to_treasury,
            to_investors,
            ..
        } = m.pay("p1", 1_000_001 + i as u128 * 7, false, T0 + i + 1)
        {
            assert_eq!(
                to_owner + to_treasury + to_investors,
                1_000_001 + i as u128 * 7
            );
            assert!(to_investors * 10_000 <= (1_000_001 + i as u128 * 7) * 333);
        }
    }
    conserved(&m);
}

#[test]
fn backdoor_proven_refunds_investor_escrow() {
    let mut m = with_dave();
    m.paused = false;
    match m.runner_guard("translator", "ed25519:hidden").unwrap() {
        RunnerCheck::BackdoorProven { .. } => {}
        r => panic!("{r:?}"),
    }
    assert!(m.frozen && m.offers.is_empty());
    assert_eq!(m.claimable_of("dave"), 250 * USD);
    conserved(&m);
}

#[test]
fn frozen_agent_sells_no_shares() {
    let mut m = agent();
    let id = m
        .offer_share("alice", 1_000, 100 * USD, T0 + YEAR, T0)
        .unwrap();
    let t = m.offer(id).unwrap().terms();
    m.paused = false;
    m.runner_guard("translator", "ed25519:hidden").unwrap();
    assert_eq!(
        m.buy_share("dave", 100 * USD, t, 1_000, T0),
        Err(Error::Frozen)
    );
}

/// Review H1: after a sale, the old owner's unsold rest of a slice closes, so
/// nobody can buy a cut of the new owner's revenue and pay the old owner.
#[test]
fn h1_sale_closes_unsold_rest_of_slices() {
    let mut m = agent();
    let a = m
        .offer_share("alice", 4_000, 100 * USD, T0 + YEAR, T0)
        .unwrap();
    let b = m
        .offer_share("alice", 500, 100 * USD, T0 + YEAR, T0)
        .unwrap();
    let ta = m.offer(a).unwrap().terms();
    m.buy_share("mule", 100 * USD, ta, 400, T0).unwrap();
    m.list("alice", 1_000 * USD, Terms::default(), "hash", T0)
        .unwrap();
    let h = m.known_keys_hash();
    let v = m.listing_version;
    let plan = m
        .begin_buy("bob", 1_000 * USD, v, &h, BOB_PK, true, T0)
        .unwrap();
    m.finish_buy(plan.sale_id, true, T0).unwrap();
    // Only the sold 4% survives; the unsold slice is gone.
    assert_eq!(m.shares_bps(), 400);
    assert!(m.offer(b).is_none());
    assert_eq!(
        m.buy_share("mule", 100 * USD, ta, 400, T0),
        Err(Error::ShareChanged)
    );
    let t_now = m.offer(a).unwrap().terms();
    assert_eq!(
        m.buy_share("mule", 100 * USD, t_now, 400, T0),
        Err(Error::OfferClosed)
    );
    conserved(&m);
}

/// Review M1: the price floor is frozen from the moment a slice is offered,
/// so the owner cannot front-run the first investor.
#[test]
fn m1_price_floor_frozen_once_offered() {
    let mut m = agent();
    m.offer_share("alice", 1_000, 100 * USD, T0 + YEAR, T0)
        .unwrap();
    let mut l = m.limits.clone();
    l.price_floor = 1_000 * USD;
    assert_eq!(m.set_limits("alice", l), Err(Error::SharesOpen));
}
