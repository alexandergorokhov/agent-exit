//! Property tests for invariants I1, I2, I3, I4, I5, I6, I7, I9, I10, I11 and S10
//! (design: threat model §3). Random action sequences run against the pure
//! machine with a fixed RNG seed, so every run checks the same cases.
//!
//! Cases: `PROP_CASES` env var (default 2000; `make verify` uses 10000). The
//! coverage gates scale with it (10 hits per 2000 cases) and hold from 200 up.

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
static LISTED_REFUSALS: AtomicUsize = AtomicUsize::new(0);
static PENDING_CALLS: AtomicUsize = AtomicUsize::new(0);
static RECOVERIES: AtomicUsize = AtomicUsize::new(0);
static FROZEN_ROLLBACKS: AtomicUsize = AtomicUsize::new(0);
static SHARE_BUYS: AtomicUsize = AtomicUsize::new(0);
static SHARE_PAYS: AtomicUsize = AtomicUsize::new(0);
static SHARE_CLOSES: AtomicUsize = AtomicUsize::new(0);

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
    /// A key batch callback (rotation or deed-transfer deletion) runs.
    KeysDone(usize, bool),
    /// A self-call signed by a key in `incoming` or `retiring` (A25).
    PendingKeyCall(bool),
    /// A key-batch callback fails after its batch ran: it never arrives (A33).
    DropKeys(usize),
    /// The buy callback fails after the key batch ran (A33).
    DropHandover,
    /// `recover_keys` by ACTORS[caller], or `recover_sale` (A33).
    Recover(usize, bool),
    /// A hidden key signs a self-call while a buy is settling (A34).
    HiddenWhileSettling,
    /// ACTORS[caller] offers bps for price (USD) until now + days.
    Offer(usize, u16, u64, u64),
    /// ACTORS[buyer] buys part of slice i; `good` = the terms it saw match.
    BuyShare(usize, usize, u16, u64, bool),
    /// Anyone closes slice i.
    CloseShare(usize),
    /// ACTORS[caller] cancels slice i.
    CancelOffer(usize, usize),
}

/// A key batch waiting for its callback.
#[derive(Debug, Clone)]
enum KeyBatch {
    Rotation {
        old: Vec<Key>,
        deleted: Vec<Key>,
        new_key: Key,
    },
    Deletion(Vec<Key>),
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
        2 => (0u64..30 * 86_400).prop_map(Act::Time),
        1 => (0usize..4).prop_map(Act::Finalize),
        1 => (a.clone(), a.clone()).prop_map(|(c, r)| Act::TransferDeed(c, r)),
        1 => (a.clone(), any::<u8>()).prop_map(|(c, k)| Act::SetKey(c, k)),
        1 => (a.clone(), a.clone()).prop_map(|(c, r)| Act::Approve(c, r)),
        1 => a.clone().prop_map(Act::Revoke),
        1 => prop::bool::weighted(0.95).prop_map(Act::Runner),
        2 => (a.clone(), 0u64..5_000_000, 0u64..80_000_000, any::<u8>(), 0u16..=10_000)
            .prop_map(|(c, f, cap, mask, t)| Act::SetLimits(c, f, cap, mask, t)),
        4 => (a.clone(), 1u64..30_000_000, prop::bool::weighted(0.995)).prop_map(|(p, x, k)| Act::Spend(p, x, k)),
        3 => (0usize..4, prop::bool::weighted(0.9)).prop_map(|(i, ok)| Act::KeysDone(i, ok)),
        1 => any::<bool>().prop_map(Act::PendingKeyCall),
        1 => (0usize..4).prop_map(Act::DropKeys),
        1 => Just(Act::DropHandover),
        1 => Just(Act::HiddenWhileSettling),
        2 => (a.clone(), any::<bool>()).prop_map(|(c, sale)| Act::Recover(c, sale)),
        2 => (a.clone(), 0u16..5_200, 0u64..2_000, 0u64..800).prop_map(|(c, b, p, d)| Act::Offer(c, b, p, d)),
        3 => (a.clone(), 0usize..3, 0u16..3_000, 0u64..3_000_000_000, prop::bool::weighted(0.9))
            .prop_map(|(b, i, part, extra, good)| Act::BuyShare(b, i, part, extra, good)),
        1 => (0usize..3).prop_map(Act::CloseShare),
        1 => (a.clone(), 0usize..3).prop_map(|(c, i)| Act::CancelOffer(c, i)),
    ]
}

struct World {
    m: Machine,
    now: Ts,
    in_flight: Vec<(TransferKind, String, Option<String>, Amount)>,
    before_buy: Option<Machine>,
    key_counter: u64,
    /// Highest daily cap in force at any point of the current (epoch, day).
    max_cap_today: Amount,
    cap_day: (u64, u64),
    key_batches: Vec<KeyBatch>,
    /// The pending buy's callback failed and will never run.
    handover_dropped: bool,
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
        m.escrow_left()
            + m.claimable_total()
            + m.owner_free
            + m.treasury
            + m.share_escrow
            + pending_amount(m)
    );
    // SH1, SH5: open slices <= 49%, at most 16 investors; each slice's parts
    // add up; share escrow is the sum of the holders' escrow.
    prop_assert!(m.shares_bps() <= shares::MAX_SHARES_BPS);
    prop_assert!(m.offers.iter().map(|o| o.holders.len()).sum::<usize>() <= shares::MAX_HOLDERS);
    for o in &m.offers {
        prop_assert_eq!(o.sold_bps, o.holders.iter().map(|h| h.bps).sum::<u16>());
        prop_assert!(o.sold_bps <= o.bps);
    }
    prop_assert_eq!(
        m.share_escrow,
        m.offers.iter().map(|o| o.escrow()).sum::<Amount>()
    );
    // I7: the runner never spent more than the cap in this (owner epoch, day),
    // unless the owner lowered the cap after the spending.
    let today = m.spent_today(w.now);
    prop_assert!(today <= w.max_cap_today.max(m.limits.daily_cap));
    // A24: while listed, the treasury never drops below the listing's snapshot.
    if let (Status::Listed, Some(l)) = (&m.status, &m.listing) {
        prop_assert!(m.treasury >= l.treasury_snapshot);
    }
    // I2: per sale, paid now + escrow = price; released + returned <= escrow.
    // Open sales in the blob; finalized ones in the history (A47).
    prop_assert!(m.sales.iter().all(|s| !s.finalized));
    prop_assert_eq!(m.closed_sales as usize, m.store.sales.len());
    for s in m.sales.iter().chain(m.store.sales.values()) {
        prop_assert_eq!(s.paid_now + s.escrow, s.price);
        prop_assert!(s.released + s.returned <= s.escrow);
        if s.finalized {
            prop_assert_eq!(s.released + s.returned, s.escrow);
        }
    }
    // I9: verified <= counted <= gross.
    let e = m.earnings(w.now);
    prop_assert!(e.verified <= e.counted && e.counted <= e.gross);
    // I11: payer rows only by registration (A49/A50), counters agree.
    prop_assert_eq!(m.store.payers.len() as u64, m.meter.tracked_payers);
    prop_assert!(m.meter.empty_payers <= m.meter.tracked_payers);
    prop_assert_eq!(
        m.store
            .payers
            .values()
            .filter(|p| p.totals.payments == 0)
            .count() as u64,
        m.meter.empty_payers
    );
    // A53: the paid_to counters agree with the stored entries.
    prop_assert_eq!(
        m.store
            .paid_to
            .values()
            .filter(|e| **e > m.paid_to_dead_upto)
            .count() as u64,
        m.paid_to_total()
    );
    prop_assert!((m.paid_to_total() as usize) <= MAX_PAID_TO_TOTAL);
    prop_assert!(m.paid_to_this_owner() <= MAX_PAID_TO);
    // A25: incoming / retiring / known never overlap.
    prop_assert!(m.incoming.is_disjoint(&m.known_keys));
    prop_assert!(m.retiring.is_disjoint(&m.known_keys));
    // Settling <=> pending.
    prop_assert_eq!(m.status == Status::Settling, m.pending.is_some());
    // I10: the owner can always take out all revenue unless a buy is settling
    // (while Listed, after cancelling the listing: STR-12).
    if m.status != Status::Settling && m.owner_free > 0 {
        let mut c = m.clone();
        let owner = c.owner.clone();
        if c.status == Status::Listed {
            prop_assert!(c.withdraw(&owner, c.owner_free, &owner).is_err());
            prop_assert!(c.cancel_listing(&owner).is_ok());
        }
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
            let payers_before = w.m.store.payers.len();
            let days_before = w.m.store.days.len();
            let r = w.m.pay(payer, amount, x % 3 == 0, w.now);
            match r {
                PayResult::Refund(_) => {
                    prop_assert!(amount < meter::MIN_PAYMENT || amount < before.limits.price_floor);
                    prop_assert_eq!(&w.m, &before);
                }
                PayResult::Recorded {
                    flags,
                    released,
                    to_owner,
                    to_treasury,
                    to_investors,
                    share_released,
                    ..
                } => {
                    // Feature 12 + SH2: the split adds up exactly and lands
                    // in the buckets; investors take from the gross (T3).
                    prop_assert_eq!(to_owner + to_treasury + to_investors, amount);
                    let to_seller_owner: Amount = share_released
                        .iter()
                        .filter(|(s, _)| *s == owner)
                        .map(|(_, r)| *r)
                        .sum();
                    prop_assert_eq!(
                        w.m.owner_free,
                        before.owner_free + to_owner + to_seller_owner
                    );
                    prop_assert_eq!(w.m.treasury, before.treasury + to_treasury);
                    prop_assert_eq!(
                        to_treasury,
                        (amount - to_investors) * before.limits.treasury_bps as Amount / 10_000
                    );
                    let promised: u128 = before
                        .offers
                        .iter()
                        .filter(|o| o.active(w.now))
                        .map(|o| o.sold_bps as u128)
                        .sum();
                    prop_assert!(to_investors * 10_000 <= amount * promised);
                    // Escrow released never exceeds what investors got (SH11).
                    let rel: Amount = share_released.iter().map(|(_, r)| *r).sum();
                    prop_assert!(rel <= to_investors);
                    prop_assert_eq!(w.m.share_escrow, before.share_escrow - rel);
                    if to_investors > 0 {
                        SHARE_PAYS.fetch_add(1, Relaxed);
                    }
                    // S10 property: release from one payment <= 50% of it.
                    let total: Amount = released.iter().map(|r| r.2).sum();
                    prop_assert!(total * 2 <= amount);
                    if total > 0 {
                        RELEASES.fetch_add(1, Relaxed);
                    }
                    // Self and sub-account payments never release; a round
                    // trip releases only if the buyer created it (A31).
                    if flags.self_pay || flags.seller_subaccount {
                        prop_assert_eq!(total, 0);
                    }
                    // I11: at most one new payer and one new day per payment.
                    prop_assert!(w.m.store.payers.len() <= payers_before + 1);
                    prop_assert!(w.m.store.days.len() <= days_before + 1);
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
            if w.handover_dropped {
                return check_invariants(w);
            }
            let seller = w.m.owner.clone();
            let r = w.m.finish_buy(p.sale_id, ok, w.now).unwrap();
            match r {
                BuyResult::Done {
                    seller: s,
                    payout,
                    seller_revenue,
                    ..
                } => {
                    // A29: the seller's unwithdrawn revenue went to the seller.
                    prop_assert_eq!(w.m.owner_free, 0);
                    prop_assert!(w.m.claimable_of(&s) >= seller_revenue);
                    prop_assert!(!w.m.incoming.contains(&p.buyer_pk));
                    // I1: paid => deed moved, seller keys gone, buyer key added.
                    prop_assert_eq!(&s, &seller);
                    prop_assert_eq!(&w.m.owner, &p.buyer);
                    prop_assert!(w.m.known_keys.len() == 1 && w.m.known_keys.contains(&p.buyer_pk));
                    for k in &p.prev_keys {
                        prop_assert!(!w.m.known_keys.contains(k));
                    }
                    SALES.fetch_add(1, Relaxed);
                    w.in_flight
                        .push((TransferKind::SellerPayout, s, None, payout));
                }
                BuyResult::RolledBack {
                    unused,
                    frozen,
                    delete_keys,
                } => {
                    // I6: full refund either way.
                    prop_assert_eq!(unused, p.amount);
                    ROLLBACKS.fetch_add(1, Relaxed);
                    if frozen {
                        // A34: frozen while settling: no sale, the seller
                        // keeps the agent, the buyer's key is being deleted.
                        let before_buy = w.before_buy.take().unwrap();
                        prop_assert!(w.m.frozen);
                        prop_assert_eq!(&w.m.owner, &seller);
                        prop_assert_eq!(
                            w.m.sales.len() + w.m.closed_sales as usize,
                            before_buy.sales.len() + before_buy.closed_sales as usize
                        );
                        prop_assert!(w.m.sales.iter().all(|s| s.id != p.sale_id));
                        prop_assert!(w.m.listing.is_none());
                        prop_assert!(
                            w.m.status != Status::Listed && w.m.status != Status::Settling
                        );
                        prop_assert_eq!(delete_keys.is_empty(), !ok);
                        prop_assert!(!w.m.known_keys.contains(&p.buyer_pk));
                        if !delete_keys.is_empty() {
                            w.key_batches.push(KeyBatch::Deletion(delete_keys));
                        }
                        FROZEN_ROLLBACKS.fetch_add(1, Relaxed);
                        return check_invariants(w);
                    }
                    prop_assert!(delete_keys.is_empty());
                    let mut expect = w.before_buy.take().unwrap();
                    expect.next_sale_id = w.m.next_sale_id;
                    // Payments may have arrived while settling; compare the rest.
                    expect.meter = w.m.meter.clone();
                    expect.store = w.m.store.clone();
                    expect.held = w.m.held;
                    expect.owner_free = w.m.owner_free;
                    expect.treasury = w.m.treasury;
                    expect.paid_to_ever = w.m.paid_to_ever;
                    expect.paid_to_this_epoch = w.m.paid_to_this_epoch;
                    expect.paid_to_dead_upto = w.m.paid_to_dead_upto;
                    expect.paid_to_dead_count = w.m.paid_to_dead_count;
                    // Only payments while settling may have changed these;
                    // the rolled-back sale left no record (A34).
                    expect.claimable_sum = w.m.claimable_sum;
                    expect.claimable_accounts = w.m.claimable_accounts;
                    expect.sales = w.m.sales.clone();
                    prop_assert!(w.m.sales.iter().all(|s| s.id != p.sale_id));
                    prop_assert!(!w.m.frozen);
                    prop_assert_eq!(&w.m, &expect);
                }
            }
        }
        Act::Resolve(i, ok) => {
            if !w.in_flight.is_empty() {
                let (k, to, by, amt) = w.in_flight.remove(i % w.in_flight.len());
                let owner_before = w.m.owner.clone();
                let claim_before = by.as_ref().map(|b| w.m.claimable_of(b));
                w.m.transfer_done(k, &to, by.as_deref(), amt, ok);
                // A38: a failed withdrawal goes back to whoever started it.
                if let (false, TransferKind::OwnerWithdraw, Some(b)) = (ok, k, &by) {
                    if *b != owner_before {
                        prop_assert_eq!(w.m.claimable_of(b), claim_before.unwrap() + amt);
                    }
                }
            }
        }
        Act::Claim(c) => {
            if let Ok(amt) = w.m.claim(ACTORS[c]) {
                w.in_flight
                    .push((TransferKind::Claim, ACTORS[c].to_string(), None, amt));
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
                w.in_flight.push((
                    TransferKind::OwnerWithdraw,
                    ACTORS[c].to_string(),
                    Some(ACTORS[c].to_string()),
                    a,
                ));
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
            let res = w.m.transfer_deed(ACTORS[c], ACTORS[r], None, w.now);
            if settling {
                prop_assert!(res.is_err());
            }
            if let Ok(keys) = &res {
                prop_assert!(w.m.known_keys.is_empty() && w.m.paused);
                prop_assert_eq!(&w.m.owner, ACTORS[r]);
                for k in keys {
                    prop_assert!(w.m.retiring.contains(k));
                }
                if !keys.is_empty() {
                    w.key_batches.push(KeyBatch::Deletion(keys.clone()));
                }
            } else {
                prop_assert_eq!(&w.m, &before);
            }
        }
        Act::SetKey(c, k) => {
            let new_key = format!("ed25519:k{k}");
            let old: Vec<Key> = w.m.known_keys.iter().cloned().collect();
            let r = w.m.set_runner_key(ACTORS[c], &new_key, w.now);
            if ACTORS[c] != owner || settling {
                prop_assert!(r.is_err());
            }
            match r {
                Err(_) => prop_assert_eq!(&w.m, &before),
                Ok(deleted) => {
                    prop_assert!(w.m.known_keys.is_empty() && w.m.incoming.contains(&new_key));
                    w.key_batches.push(KeyBatch::Rotation {
                        old,
                        deleted,
                        new_key,
                    });
                }
            }
        }
        Act::KeysDone(i, ok) => {
            if !w.key_batches.is_empty() {
                match w.key_batches.remove(i % w.key_batches.len()) {
                    KeyBatch::Rotation {
                        old,
                        deleted,
                        new_key,
                    } => {
                        {
                            let b = w.m.key_batch;
                            w.m.runner_key_done(b, &old, &deleted, &new_key, ok);
                        }
                        // An honest rotation never bricks the agent: the
                        // runner key works again (success, or the old keys
                        // restored), or the owner can rotate again.
                        prop_assert!(!w.m.incoming.contains(&new_key));
                        if ok {
                            prop_assert!(w.m.known_keys.contains(&new_key));
                        } else {
                            for k in &old {
                                prop_assert!(w.m.known_keys.contains(k));
                            }
                        }
                        if !w.m.frozen
                            && w.m.status != Status::Settling
                            && w.m.status != Status::Listed
                            && w.m.incoming.is_empty()
                            && w.m.retiring.is_empty()
                        {
                            let mut c = w.m.clone();
                            let owner = c.owner.clone();
                            prop_assert!(c.set_runner_key(&owner, "ed25519:retry", w.now).is_ok());
                        }
                    }
                    KeyBatch::Deletion(keys) => w.m.keys_deleted(&keys),
                }
            }
        }
        Act::PendingKeyCall(incoming) => {
            let pk = if incoming {
                w.m.incoming.iter().next().cloned()
            } else {
                w.m.retiring.iter().next().cloned()
            };
            if let Some(pk) = pk {
                // A25: never a freeze, never a refund.
                let r = w.m.runner_guard("translator", &pk);
                PENDING_CALLS.fetch_add(1, Relaxed);
                if !before.frozen {
                    prop_assert_eq!(r, Err(Error::NotRunner));
                }
                prop_assert_eq!(&w.m, &before);
            }
        }
        Act::DropKeys(i) => {
            if !w.key_batches.is_empty() {
                w.key_batches.remove(i % w.key_batches.len());
            }
        }
        Act::Offer(c, bps, price, days) => {
            let r = w.m.offer_share(
                ACTORS[c],
                bps,
                price as Amount * USD,
                w.now + days * DAY_NS,
                w.now,
            );
            check_owner_only(ACTORS[c], &owner, settling, r.is_ok(), &before, &w.m)?;
            if r.is_ok() {
                prop_assert!(bps >= shares::MIN_OFFER_BPS && w.m.status != Status::Listed);
            }
        }
        Act::BuyShare(b, i, part, extra, good) => {
            let buyer = ACTORS[b];
            if let Some(o) = w.m.offers.get(i).cloned() {
                let mut t = o.terms();
                if !good {
                    t.price += 1;
                }
                let cost = if part > 0 { o.cost(part) } else { 0 };
                let amount = cost + extra as Amount;
                let held = w.m.held;
                match w.m.buy_share(buyer, amount, t, part, w.now) {
                    Ok(c) => {
                        SHARE_BUYS.fetch_add(1, Relaxed);
                        prop_assert!(good && c == cost && c <= amount);
                        prop_assert!(buyer != owner && !w.m.frozen);
                        prop_assert!(matches!(w.m.status, Status::Active | Status::Sold));
                        prop_assert_eq!(w.m.held, held + c);
                        let (_, esc) = shares::split_cost(c);
                        prop_assert_eq!(w.m.share_escrow, before.share_escrow + esc);
                    }
                    Err(_) => prop_assert_eq!(&w.m, &before),
                }
            }
        }
        Act::CloseShare(i) => {
            if let Some(id) = w.m.offers.get(i).map(|o| o.id) {
                let esc = w.m.offers[i].escrow();
                let claim = w.m.claimable_total();
                match w.m.close_share(id, w.now) {
                    Ok(refunds) => {
                        SHARE_CLOSES.fetch_add(1, Relaxed);
                        let total: Amount = refunds.iter().map(|r| r.1).sum();
                        prop_assert_eq!(total, esc);
                        prop_assert_eq!(w.m.claimable_total(), claim + esc);
                    }
                    Err(_) => prop_assert_eq!(&w.m, &before),
                }
            }
        }
        Act::CancelOffer(c, i) => {
            if let Some(o) = w.m.offers.get(i).cloned() {
                let r = w.m.cancel_offer(ACTORS[c], o.id);
                if r.is_ok() {
                    prop_assert!(ACTORS[c] == owner && o.sold_bps == 0);
                } else {
                    prop_assert_eq!(&w.m, &before);
                }
            }
        }
        Act::HiddenWhileSettling => {
            if settling {
                return step(w, &Act::Runner(false));
            }
        }
        Act::DropHandover => {
            if settling {
                w.handover_dropped = true;
            }
        }
        Act::Recover(c, sale) => {
            // Recovery is only reachable once no callback can still arrive
            // (an hour later in reality): skip it while one is in flight.
            if sale {
                if !(settling && w.handover_dropped) {
                    return check_invariants(w);
                }
                let p = w.m.pending.clone().unwrap();
                if w.now < p.started_at + RECOVER_AFTER_NS {
                    prop_assert_eq!(w.m.clone().recover_sale(w.now), Err(Error::RecoverTooEarly));
                }
                w.now += RECOVER_AFTER_NS;
                let r = w.m.recover_sale(w.now).unwrap();
                RECOVERIES.fetch_add(1, Relaxed);
                w.handover_dropped = false;
                w.before_buy = None;
                // The sale did not happen: the seller keeps the deed, no
                // sale record, every key the buy touched is being deleted.
                prop_assert_eq!(&w.m.owner, &before.owner);
                prop_assert!(w.m.pending.is_none() && w.m.listing.is_none());
                prop_assert!(w.m.status != Status::Listed && w.m.status != Status::Settling);
                prop_assert_eq!(
                    w.m.sales.len() + w.m.closed_sales as usize,
                    before.sales.len() + before.closed_sales as usize
                );
                prop_assert!(w.m.known_keys.is_empty() && w.m.paused);
                prop_assert!(r.delete_keys.contains(&p.buyer_pk));
                for k in &p.prev_keys {
                    prop_assert!(r.delete_keys.contains(k));
                }
                w.key_batches.push(KeyBatch::Deletion(r.delete_keys));
            } else {
                if !w.key_batches.is_empty() || settling {
                    return check_invariants(w);
                }
                let stuck = !w.m.incoming.is_empty() || !w.m.retiring.is_empty();
                let caller = ACTORS[c];
                w.now += RECOVER_AFTER_NS;
                let r = w.m.recover_keys(caller, w.now);
                if caller != owner || !stuck {
                    prop_assert!(r.is_err());
                    prop_assert_eq!(&w.m.known_keys, &before.known_keys);
                    return check_invariants(w);
                }
                let r = r.unwrap();
                RECOVERIES.fetch_add(1, Relaxed);
                prop_assert!(w.m.incoming.is_empty());
                prop_assert_eq!(&w.m.known_keys, &before.known_keys);
                w.m.keys_deleted(&r.delete_keys);
                // Recovered: nothing pending, and the owner can rotate again
                // (unless frozen or listed).
                prop_assert!(w.m.retiring.is_empty());
                if !w.m.frozen && w.m.status != Status::Listed {
                    let mut c = w.m.clone();
                    prop_assert!(c.set_runner_key(&owner, "ed25519:retry", w.now).is_ok());
                }
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
                // Every open earn-out was cut and moved to the history.
                prop_assert!(w.m.sales.is_empty());
                for s in w.m.store.sales.values() {
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
            if known && w.m.known_keys.is_empty() {
                // No runner key exists (deed just moved): nobody to spend.
                return check_invariants(w);
            }
            let pk = if known {
                w.m.known_keys.iter().next().cloned().unwrap_or_default()
            } else {
                "ed25519:hidden".to_string()
            };
            if let Ok(RunnerCheck::Ok) = w.m.runner_guard("translator", &pk) {
                let payee = ACTORS[p];
                let amount = x as Amount;
                let spent_before = w.m.spent_today(w.now);
                let r = w.m.spend(payee, amount, w.now);
                if before.status == Status::Listed {
                    // A24: spending is paused while listed.
                    prop_assert_eq!(r.clone().unwrap_err(), Error::SpendingPaused);
                    LISTED_REFUSALS.fetch_add(1, Relaxed);
                }
                match r {
                    Ok(total) => {
                        SPENDS.fetch_add(1, Relaxed);
                        // I7: inside the payees, the cap and the treasury.
                        prop_assert!(before.limits.allowed_payees.contains(payee));
                        prop_assert_eq!(total, spent_before + amount);
                        prop_assert!(total <= before.limits.daily_cap);
                        prop_assert!(amount <= before.treasury);
                        prop_assert_eq!(w.m.treasury, before.treasury - amount);
                        w.in_flight.push((
                            TransferKind::TreasuryPay,
                            payee.to_string(),
                            None,
                            amount,
                        ));
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

fn cases() -> u32 {
    std::env::var("PROP_CASES")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(2_000)
}

fn runner() -> TestRunner {
    let cfg = Config {
        cases: cases(),
        failure_persistence: None,
        ..Config::default()
    };
    TestRunner::new_with_rng(cfg, TestRng::from_seed(RngAlgorithm::ChaCha, &[42u8; 32]))
}

#[test]
fn prop_invariants_random_sequences() {
    let mut r = runner();
    r.run(&prop::collection::vec(act(), 1..120), |acts| {
        let mut m = Machine::new("translator", "alice");
        let deleted = m
            .set_runner_key("alice", "ed25519:alice-runner", T0)
            .unwrap();
        m.runner_key_done(m.key_batch, &[], &deleted, "ed25519:alice-runner", true);
        // Half the actors registered a payer row (A49); the rest are pooled.
        for a in ACTORS.iter().step_by(2) {
            m.register_payer(a);
        }
        let mut w = World {
            m,
            now: T0,
            in_flight: vec![],
            before_buy: None,
            key_counter: 0,
            max_cap_today: 0,
            cap_day: (0, 0),
            key_batches: vec![],
            handover_dropped: false,
        };
        for a in &acts {
            step(&mut w, a)?;
        }
        // Every sequence ends past every earn-out window and closes each one,
        // so finalization is covered at any PROP_CASES.
        step(&mut w, &Act::Time(366 * 86_400))?;
        // Finalizing moves a sale out of `sales`, so always take the first.
        for _ in 0..w.m.sales.len() {
            step(&mut w, &Act::Finalize(0))?;
        }
        prop_assert!(w.m.sales.is_empty());
        // Every slice ends within 800 days: close them all; the share escrow
        // is then fully back with investors or sellers.
        step(&mut w, &Act::Time(800 * 86_400))?;
        for _ in 0..w.m.offers.len() {
            step(&mut w, &Act::CloseShare(0))?;
        }
        prop_assert!(w.m.offers.is_empty());
        prop_assert_eq!(w.m.share_escrow, 0);
        Ok(())
    })
    .unwrap();
    // The gates scale with the case count (10 hits per 2000 cases, at least 1).
    let need = (10 * cases() as usize / 2_000).clamp(1, 10);
    for (name, c) in [
        ("sales", &SALES),
        ("rollbacks", &ROLLBACKS),
        ("backdoors", &BACKDOORS),
        ("releases", &RELEASES),
        ("finalized", &FINALIZED),
        ("treasury spends", &SPENDS),
        ("cap refusals", &CAP_HITS),
        ("spends refused while listed", &LISTED_REFUSALS),
        ("pending-key calls", &PENDING_CALLS),
        ("recoveries", &RECOVERIES),
        ("frozen while settling", &FROZEN_ROLLBACKS),
        ("share buys", &SHARE_BUYS),
        ("payments to investors", &SHARE_PAYS),
        ("share slices closed", &SHARE_CLOSES),
    ] {
        let n = c.load(Relaxed);
        eprintln!("coverage {name}: {n}");
        assert!(
            n >= need,
            "property test reached {name} {n} times, needs {need}"
        );
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
    // E16 / I11: payments never add rows; only registered payers have one.
    let mut m = Machine::new("translator", "alice");
    for i in 0..1_005 {
        if i < 1_000 {
            assert!(m.register_payer(&format!("payer{i}")));
        }
        m.pay(&format!("payer{i}"), USD, false, T0);
    }
    assert_eq!(m.store.payers.len(), 1_000);
    assert_eq!(m.meter.other.totals.payments, 5);
    let e = m.earnings(T0);
    assert_eq!(e.gross, 1_005 * USD);
    assert_eq!(e.other_payers_gross, 5 * USD);
}

/// Buyer-side actions after a sale (A31).
#[derive(Debug, Clone)]
enum PostSale {
    /// An honest customer pays.
    Customer(usize, u64),
    /// The buyer withdraws revenue to a customer (makes it a round trip).
    WithdrawTo(usize, u64),
    /// The buyer's runner pays a customer from the treasury.
    SpendTo(usize, u64),
    Time(u64),
}

const CUSTOMERS: [&str; 4] = ["c1.near", "c2.near", "c3.near", "c4.near"];

/// A31 (review 2, H1): the seller's earn-out is never below what the honest
/// customer payments alone release. Twin machines see the same customer
/// payments and clock; one also runs arbitrary buyer withdraws and treasury
/// payments to those customers. The seller's release (during the window and
/// at finalization) must be the same.
#[test]
fn prop_buyer_cannot_shrink_earnout() {
    let mut r = runner();
    let post = prop_oneof![
        5 => (0usize..4, 100_000u64..50_000_000).prop_map(|(c, x)| PostSale::Customer(c, x)),
        2 => (0usize..4, 1u64..2_000_000).prop_map(|(c, x)| PostSale::WithdrawTo(c, x)),
        2 => (0usize..4, 1u64..2_000_000).prop_map(|(c, x)| PostSale::SpendTo(c, x)),
        1 => (0u64..10 * 86_400).prop_map(PostSale::Time),
    ];
    r.run(&prop::collection::vec(post, 1..60), |acts| {
        let mut m = Machine::new("translator", "alice");
        let d = m.set_runner_key("alice", "ed25519:a", T0).unwrap();
        m.runner_key_done(m.key_batch, &[], &d, "ed25519:a", true);
        // Pre-sale history so the seller has round trips of their own.
        m.pay("c1.near", 5 * USD, false, T0);
        m.withdraw("alice", USD, "c4.near").unwrap();
        m.list("alice", 1_000 * USD, Terms::default(), "h", T0)
            .unwrap();
        let h = m.known_keys_hash();
        let v = m.listing_version;
        let plan = m
            .begin_buy("bob", 1_000 * USD, v, &h, "ed25519:b", true, T0)
            .unwrap();
        m.finish_buy(plan.sale_id, true, T0).unwrap();
        m.set_limits(
            "bob",
            Limits {
                price_floor: 0,
                daily_cap: 1_000 * USD,
                allowed_payees: CUSTOMERS.iter().map(|c| c.to_string()).collect(),
                owner_bps: 5_000,
                treasury_bps: 5_000,
            },
        )
        .unwrap();
        let mut honest = m.clone();
        let mut now = T0;
        for a in &acts {
            match a {
                PostSale::Customer(c, x) => {
                    m.pay(CUSTOMERS[*c], *x as Amount, false, now);
                    honest.pay(CUSTOMERS[*c], *x as Amount, false, now);
                }
                PostSale::WithdrawTo(c, x) => {
                    let _ = m.withdraw("bob", (*x as Amount).min(m.owner_free), CUSTOMERS[*c]);
                }
                PostSale::SpendTo(c, x) => {
                    if m.runner_guard("translator", "ed25519:b").is_ok() {
                        let _ = m.spend(CUSTOMERS[*c], *x as Amount, now);
                    }
                }
                PostSale::Time(t) => now += t * NANOS_PER_SEC,
            }
            prop_assert_eq!(m.sales[0].released, honest.sales[0].released);
        }
        now += 31 * DAY_NS;
        let got = m.finalize_earnout(1, now).unwrap();
        let want = honest.finalize_earnout(1, now).unwrap();
        prop_assert!(got.released >= want.released);
        prop_assert_eq!(got.released, want.released);
        Ok(())
    })
    .unwrap();
}
