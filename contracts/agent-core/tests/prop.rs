//! Property tests for invariants I1, I2, I3, I4, I5, I6, I7, I9, I10, I11 and S10
//! (design: threat model §3). Random action sequences run against the pure
//! machine with a fixed RNG seed, so every run checks the same cases.
//!
//! Cases: `PROP_CASES` env var (default 2000; `make verify` uses 10000).

use agent_core::*;
use proptest::prelude::*;
use proptest::test_runner::{Config, RngAlgorithm, TestRng, TestRunner};
use std::sync::atomic::{AtomicUsize, Ordering::Relaxed};

// Coverage counters: the property test fails if the random sequences never
// reach the interesting paths (a vacuous pass).
static SALES: AtomicUsize = AtomicUsize::new(0);
static ROLLBACKS: AtomicUsize = AtomicUsize::new(0);
static BACKDOORS: AtomicUsize = AtomicUsize::new(0);
static RELEASES: AtomicUsize = AtomicUsize::new(0);
static FINALIZED: AtomicUsize = AtomicUsize::new(0);
static SPENDS: AtomicUsize = AtomicUsize::new(0);
static CAP_HITS: AtomicUsize = AtomicUsize::new(0);

const ACTORS: [&str; 9] = [
    "alice",
    "bob",
    "carol",
    "mallory",
    "admin",
    "translator",
    "p1",
    "p2",
    "x.alice",
];
const T0: Ts = 1_767_225_600 * NANOS_PER_SEC;

#[derive(Debug, Clone)]
enum Act {
    Pay(usize, u64),
    List(usize, u64),
    Cancel(usize),
    Buy {
        buyer: usize,
        extra: u64,
        short: bool,
        good_version: bool,
        good_hash: bool,
        gas_ok: bool,
    },
    Finish(bool),
    Resolve(usize, bool),
    Claim(usize),
    Withdraw(usize, u8),
    Time(u64),
    Finalize(usize),
    TransferDeed(usize, usize),
    SetKey(usize, u8),
    Approve(usize, usize),
    Revoke(usize),
    Runner(bool),
    /// Owner (or someone else) sets limits: caller, floor, cap, payees mask, treasury bps.
    SetLimits(usize, u64, u64, u8, u16),
    /// Runner pays ACTORS[payee] from the treasury; `known` = signed by the runner key.
    Spend(usize, u64, bool),
}

fn act() -> impl Strategy<Value = Act> {
    let a = 0..ACTORS.len();
    prop_oneof![
        6 => (a.clone(), 0u64..60_000_000).prop_map(|(p, x)| Act::Pay(p, x)),
        2 => (a.clone(), 1u64..3_000).prop_map(|(c, p)| Act::List(c, p)),
        1 => a.clone().prop_map(Act::Cancel),
        3 => (a.clone(), 0u64..2_000_000, any::<bool>(), prop::bool::weighted(0.9), prop::bool::weighted(0.9), prop::bool::weighted(0.9))
            .prop_map(|(buyer, extra, short, good_version, good_hash, gas_ok)| Act::Buy { buyer, extra, short: short && extra % 7 == 0, good_version, good_hash, gas_ok }),
        3 => prop::bool::weighted(0.8).prop_map(Act::Finish),
        2 => (0usize..8, prop::bool::weighted(0.7)).prop_map(|(i, ok)| Act::Resolve(i, ok)),
        1 => a.clone().prop_map(Act::Claim),
        1 => (a.clone(), any::<u8>()).prop_map(|(c, f)| Act::Withdraw(c, f)),
        2 => (0u64..20 * 86_400).prop_map(Act::Time),
        1 => (0usize..4).prop_map(Act::Finalize),
        1 => (a.clone(), a.clone()).prop_map(|(c, r)| Act::TransferDeed(c, r)),
        1 => (a.clone(), any::<u8>()).prop_map(|(c, k)| Act::SetKey(c, k)),
        1 => (a.clone(), a.clone()).prop_map(|(c, r)| Act::Approve(c, r)),
        1 => a.clone().prop_map(Act::Revoke),
        1 => prop::bool::weighted(0.95).prop_map(Act::Runner),
        2 => (a.clone(), 0u64..5_000_000, 0u64..80_000_000, any::<u8>(), 0u16..=10_000)
            .prop_map(|(c, f, cap, mask, t)| Act::SetLimits(c, f, cap, mask, t)),
        4 => (a.clone(), 1u64..30_000_000, prop::bool::weighted(0.97)).prop_map(|(p, x, k)| Act::Spend(p, x, k)),
    ]
}

struct World {
    m: Machine,
    now: Ts,
    in_flight: Vec<(TransferKind, String, Amount)>,
    before_buy: Option<Machine>,
    key_counter: u64,
    /// Highest daily cap in force at any point of the current (epoch, day).
    max_cap_today: Amount,
    cap_day: (u64, u64),
}

fn pending_amount(m: &Machine) -> Amount {
    m.pending.as_ref().map(|p| p.amount).unwrap_or(0)
}

fn check_invariants(w: &World) -> Result<(), TestCaseError> {
    let m = &w.m;
    // I3 (exact form): held = escrow left + claimable + owner revenue +
    // treasury + buy in flight.
    prop_assert_eq!(
        m.held,
        m.escrow_left() + m.claimable_total() + m.owner_free + m.treasury + pending_amount(m)
    );
    // I7: the runner never spent more than the cap in this (owner epoch, day),
    // unless the owner lowered the cap after the spending.
    let today = m.spent_today(w.now);
    prop_assert!(today <= w.max_cap_today.max(m.limits.daily_cap));
    // I2: per sale, paid now + escrow = price; released + returned <= escrow.
    for s in &m.sales {
        prop_assert_eq!(s.paid_now + s.escrow, s.price);
        prop_assert!(s.released + s.returned <= s.escrow);
        if s.finalized {
            prop_assert_eq!(s.released + s.returned, s.escrow);
        }
    }
    // I9: verified <= counted <= gross.
    let e = m.earnings(w.now);
    prop_assert!(e.verified <= e.counted && e.counted <= e.gross);
    // I11: payer map bounded.
    prop_assert!(m.meter.payers.len() <= meter::MAX_PAYERS);
    prop_assert!(m.paid_to.len() <= MAX_PAID_TO);
    // Settling <=> pending.
    prop_assert_eq!(m.status == Status::Settling, m.pending.is_some());
    // I10: the owner can always take out all revenue unless a buy is settling.
    if m.status != Status::Settling && m.owner_free > 0 {
        let mut c = m.clone();
        let owner = c.owner.clone();
        prop_assert!(c.withdraw(&owner, c.owner_free, &owner).is_ok());
    }
    Ok(())
}

fn step(w: &mut World, a: &Act) -> Result<(), TestCaseError> {
    let settling = w.m.status == Status::Settling;
    let before = w.m.clone();
    let owner = w.m.owner.clone();
    let day_key = (w.m.owner_epoch, w.now / DAY_NS);
    if day_key != w.cap_day {
        w.cap_day = day_key;
        w.max_cap_today = w.m.limits.daily_cap;
    }
    match a.clone() {
        Act::Pay(p, x) => {
            let payer = ACTORS[p];
            let amount = x as Amount;
            let payers_before = w.m.meter.payers.len();
            let days_before = w.m.meter.days.len();
            let r = w.m.pay(payer, amount, x % 3 == 0, w.now);
            match r {
                PayResult::Refund(_) => {
                    prop_assert!(amount < meter::MIN_PAYMENT || amount < before.limits.price_floor);
                    prop_assert_eq!(&w.m, &before);
                }
                PayResult::Recorded {
                    counted,
                    released,
                    to_owner,
                    to_treasury,
                    ..
                } => {
                    // Feature 12: the split adds up exactly and lands in the buckets.
                    prop_assert_eq!(to_owner + to_treasury, amount);
                    prop_assert_eq!(w.m.owner_free, before.owner_free + to_owner);
                    prop_assert_eq!(w.m.treasury, before.treasury + to_treasury);
                    prop_assert_eq!(
                        to_treasury,
                        amount * before.limits.treasury_bps as Amount / 10_000
                    );
                    // S10 property: release from one payment <= 50% of it.
                    let total: Amount = released.iter().map(|r| r.2).sum();
                    prop_assert!(total * 2 <= amount);
                    if total > 0 {
                        RELEASES.fetch_add(1, Relaxed);
                    }
                    if !counted {
                        prop_assert_eq!(total, 0);
                    }
                    // I11: at most one new payer and one new day per payment.
                    prop_assert!(w.m.meter.payers.len() <= payers_before + 1);
                    prop_assert!(w.m.meter.days.len() <= days_before + 1);
                }
            }
        }
        Act::List(c, p) => {
            let r =
                w.m.list(ACTORS[c], p as Amount * USD, Terms::default(), "h", w.now);
            check_owner_only(ACTORS[c], &owner, settling, r.is_ok(), &before, &w.m)?;
        }
        Act::Cancel(c) => {
            let r = w.m.cancel_listing(ACTORS[c]);
            check_owner_only(ACTORS[c], &owner, settling, r.is_ok(), &before, &w.m)?;
        }
        Act::Buy {
            buyer,
            extra,
            short,
            good_version,
            good_hash,
            gas_ok,
        } => {
            let price = w.m.listing.as_ref().map(|l| l.price).unwrap_or(USD);
            let amount = if short {
                price - 1
            } else {
                price + extra as Amount
            };
            let v = w.m.listing_version + if good_version { 0 } else { 1 };
            let h = if good_hash {
                w.m.known_keys_hash()
            } else {
                "bad".into()
            };
            w.key_counter += 1;
            let pk = format!("ed25519:buyer-{}", w.key_counter);
            let r =
                w.m.begin_buy(ACTORS[buyer], amount, v, &h, &pk, gas_ok, w.now);
            if settling {
                // I5: no second buy while settling.
                prop_assert_eq!(r.clone().unwrap_err(), Error::SaleInProgress);
            }
            match r {
                Ok(_) => {
                    prop_assert!(good_version && good_hash && gas_ok && !short);
                    w.before_buy = Some(before);
                }
                Err(_) => prop_assert_eq!(&w.m, &before),
            }
        }
        Act::Finish(ok) => {
            let Some(p) = w.m.pending.clone() else {
                return Ok(());
            };
            let seller = w.m.owner.clone();
            let r = w.m.finish_buy(p.sale_id, ok, w.now).unwrap();
            match r {
                BuyResult::Done {
                    seller: s, payout, ..
                } => {
                    // I1: paid => deed moved, seller keys gone, buyer key added.
                    prop_assert_eq!(&s, &seller);
                    prop_assert_eq!(&w.m.owner, &p.buyer);
                    prop_assert!(w.m.known_keys.len() == 1 && w.m.known_keys.contains(&p.buyer_pk));
                    for k in &p.prev_keys {
                        prop_assert!(!w.m.known_keys.contains(k));
                    }
                    SALES.fetch_add(1, Relaxed);
                    w.in_flight.push((TransferKind::SellerPayout, s, payout));
                }
                BuyResult::RolledBack { unused } => {
                    // I6: state as before the buy (only the sale id was used),
                    // full refund.
                    prop_assert_eq!(unused, p.amount);
                    ROLLBACKS.fetch_add(1, Relaxed);
                    let mut expect = w.before_buy.take().unwrap();
                    expect.next_sale_id = w.m.next_sale_id;
                    // Payments may have arrived while settling; compare the rest.
                    expect.meter = w.m.meter.clone();
                    expect.held = w.m.held;
                    expect.owner_free = w.m.owner_free;
                    expect.treasury = w.m.treasury;
                    expect.claimable = w.m.claimable.clone();
                    expect.sales = w.m.sales.clone();
                    expect.frozen = w.m.frozen;
                    prop_assert_eq!(&w.m, &expect);
                }
            }
        }
        Act::Resolve(i, ok) => {
            if !w.in_flight.is_empty() {
                let (k, to, amt) = w.in_flight.remove(i % w.in_flight.len());
                w.m.transfer_done(k, &to, amt, ok);
            }
        }
        Act::Claim(c) => {
            if let Ok(amt) = w.m.claim(ACTORS[c]) {
                w.in_flight
                    .push((TransferKind::Claim, ACTORS[c].to_string(), amt));
            }
        }
        Act::Withdraw(c, f) => {
            let amt = w.m.owner_free * (f as Amount) / 255;
            let r = w.m.withdraw(ACTORS[c], amt, ACTORS[c]);
            if ACTORS[c] != owner || settling {
                prop_assert!(r.is_err());
                prop_assert_eq!(&w.m, &before);
            }
            if let Ok(a) = r {
                w.in_flight
                    .push((TransferKind::OwnerWithdraw, ACTORS[c].to_string(), a));
            }
        }
        Act::Time(s) => w.now += s * NANOS_PER_SEC,
        Act::Finalize(i) => {
            if let Some(id) = w.m.sales.get(i).map(|s| s.id) {
                if w.m.finalize_earnout(id, w.now).is_ok() {
                    FINALIZED.fetch_add(1, Relaxed);
                }
            }
        }
        Act::TransferDeed(c, r) => {
            let res = w.m.transfer_deed(ACTORS[c], ACTORS[r], None);
            if settling {
                prop_assert!(res.is_err());
            }
            if res.is_ok() {
                prop_assert!(w.m.known_keys.is_empty() && w.m.paused);
                prop_assert_eq!(&w.m.owner, ACTORS[r]);
            } else {
                prop_assert_eq!(&w.m, &before);
            }
        }
        Act::SetKey(c, k) => {
            let r = w.m.set_runner_key(ACTORS[c], &format!("ed25519:k{k}"));
            if ACTORS[c] != owner || settling {
                prop_assert!(r.is_err());
            }
            if r.is_err() {
                prop_assert_eq!(&w.m, &before);
            }
        }
        Act::Approve(c, r) => {
            let res = w.m.approve(ACTORS[c], ACTORS[r]);
            check_owner_only(ACTORS[c], &owner, settling, res.is_ok(), &before, &w.m)?;
        }
        Act::Revoke(c) => {
            let res = w.m.revoke(ACTORS[c], None);
            if ACTORS[c] != owner {
                prop_assert!(res.is_err());
            }
        }
        Act::Runner(known) => {
            let pk = if known {
                w.m.known_keys.iter().next().cloned().unwrap_or_default()
            } else {
                "ed25519:hidden".to_string()
            };
            let r = w.m.runner_guard("translator", &pk);
            if settling && known {
                // I5: no runner action while settling.
                prop_assert!(!matches!(r, Ok(RunnerCheck::Ok)));
            }
            if let Ok(RunnerCheck::BackdoorProven { .. }) = r {
                prop_assert!(w.m.frozen);
                BACKDOORS.fetch_add(1, Relaxed);
                for s in &w.m.sales {
                    prop_assert!(s.finalized);
                }
            }
            // I4: an outsider is never a runner.
            prop_assert_eq!(
                w.m.clone().runner_guard("mallory", &pk),
                Err(Error::NotRunner)
            );
        }
        Act::SetLimits(c, floor, cap, mask, t) => {
            let payees = (0..ACTORS.len())
                .filter(|i| mask & (1 << (i % 8)) != 0)
                .map(|i| ACTORS[i].to_string())
                .collect();
            let limits = Limits {
                price_floor: floor as Amount,
                daily_cap: cap as Amount,
                allowed_payees: payees,
                owner_bps: 10_000 - t,
                treasury_bps: t,
            };
            let ok_shape = limits.check("translator").is_ok();
            let r = w.m.set_limits(ACTORS[c], limits.clone());
            check_owner_only(ACTORS[c], &owner, settling, r.is_ok(), &before, &w.m)?;
            if r.is_ok() {
                prop_assert!(ok_shape);
                prop_assert_eq!(&w.m.limits, &limits);
                w.max_cap_today = w.max_cap_today.max(limits.daily_cap);
            }
        }
        Act::Spend(p, x, known) => {
            let pk = if known {
                w.m.known_keys.iter().next().cloned().unwrap_or_default()
            } else {
                "ed25519:hidden".to_string()
            };
            if let Ok(RunnerCheck::Ok) = w.m.runner_guard("translator", &pk) {
                let payee = ACTORS[p];
                let amount = x as Amount;
                let spent_before = w.m.spent_today(w.now);
                match w.m.spend(payee, amount, w.now) {
                    Ok(total) => {
                        SPENDS.fetch_add(1, Relaxed);
                        // I7: inside the payees, the cap and the treasury.
                        prop_assert!(before.limits.allowed_payees.contains(payee));
                        prop_assert_eq!(total, spent_before + amount);
                        prop_assert!(total <= before.limits.daily_cap);
                        prop_assert!(amount <= before.treasury);
                        prop_assert_eq!(w.m.treasury, before.treasury - amount);
                        w.in_flight
                            .push((TransferKind::TreasuryPay, payee.to_string(), amount));
                    }
                    Err(e) => {
                        if e == Error::PolicyViolation(Policy::DailyCap) {
                            CAP_HITS.fetch_add(1, Relaxed);
                            prop_assert!(spent_before + amount > before.limits.daily_cap);
                        }
                        let mut expect = before.clone();
                        expect.frozen = w.m.frozen;
                        prop_assert_eq!(&w.m, &expect);
                    }
                }
            }
        }
    }
    check_invariants(w)
}

/// I4 + I5: owner methods succeed only for the owner, never while settling,
/// and a refused call changes nothing.
fn check_owner_only(
    caller: &str,
    owner: &str,
    settling: bool,
    ok: bool,
    before: &Machine,
    after: &Machine,
) -> Result<(), TestCaseError> {
    if caller != owner || settling {
        prop_assert!(
            !ok,
            "{caller} succeeded (owner {owner}, settling {settling})"
        );
    }
    if !ok {
        prop_assert_eq!(after, before);
    }
    Ok(())
}

fn runner() -> TestRunner {
    let cases = std::env::var("PROP_CASES")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(2_000);
    let cfg = Config {
        cases,
        failure_persistence: None,
        ..Config::default()
    };
    TestRunner::new_with_rng(cfg, TestRng::from_seed(RngAlgorithm::ChaCha, &[42u8; 32]))
}

#[test]
fn prop_invariants_random_sequences() {
    let mut r = runner();
    r.run(&prop::collection::vec(act(), 1..80), |acts| {
        let mut m = Machine::new("translator", "alice");
        m.set_runner_key("alice", "ed25519:alice-runner").unwrap();
        let mut w = World {
            m,
            now: T0,
            in_flight: vec![],
            before_buy: None,
            key_counter: 0,
            max_cap_today: 0,
            cap_day: (0, 0),
        };
        for a in &acts {
            step(&mut w, a)?;
        }
        Ok(())
    })
    .unwrap();
    for (name, c) in [
        ("sales", &SALES),
        ("rollbacks", &ROLLBACKS),
        ("backdoors", &BACKDOORS),
        ("releases", &RELEASES),
        ("finalized", &FINALIZED),
        ("treasury spends", &SPENDS),
        ("cap refusals", &CAP_HITS),
    ] {
        let n = c.load(Relaxed);
        eprintln!("coverage {name}: {n}");
        assert!(n >= 10, "property test never reached {name} ({n})");
    }
}

#[test]
fn prop_split_exact() {
    let mut r = runner();
    r.run(&(1u128..u64::MAX as u128, 0u16..=5_000), |(price, bps)| {
        let (now, escrow) = money::split_price(price, bps);
        prop_assert_eq!(now + escrow, price);
        prop_assert!(escrow >= money::mul_bps(price, bps));
        Ok(())
    })
    .unwrap();
}

/// Feature 12: owner part + treasury part == payment, for any split.
#[test]
fn prop_split_revenue_exact() {
    let mut r = runner();
    r.run(&(0u128..u64::MAX as u128, 0u16..=10_000), |(amount, t)| {
        let (owner, treasury) = money::split_revenue(amount, t);
        prop_assert_eq!(owner + treasury, amount);
        prop_assert_eq!(treasury, money::mul_bps(amount, t));
        Ok(())
    })
    .unwrap();
}

#[test]
fn prop_release_le_half() {
    let mut r = runner();
    r.run(
        &(
            0u128..u64::MAX as u128,
            1u16..=5_000,
            0u128..u64::MAX as u128,
        ),
        |(pay, share, remaining)| {
            let rel = money::release_for_payment(pay, share, remaining);
            prop_assert!(rel * 2 <= pay);
            prop_assert!(rel <= remaining);
            Ok(())
        },
    )
    .unwrap();
}

#[test]
fn prop_storage_bounded() {
    // E16 / I11: after MAX_PAYERS distinct payers, new ones pool into `other`.
    let mut m = Machine::new("translator", "alice");
    for i in 0..(meter::MAX_PAYERS + 5) {
        m.pay(&format!("payer{i}"), USD, false, T0);
    }
    assert_eq!(m.meter.payers.len(), meter::MAX_PAYERS);
    assert_eq!(m.meter.other.totals.payments, 5);
    let e = m.earnings(T0);
    assert_eq!(e.gross, (meter::MAX_PAYERS as Amount + 5) * USD);
    assert_eq!(e.other_payers_gross, 5 * USD);
}
