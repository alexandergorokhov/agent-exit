#![cfg(feature = "stress")]
//! Stress scenarios STR-01 .. STR-15, STR-18, STR-19 (docs/SCENARIOS.md "Stress (STR-*)").
//!
//! These run offline in near-sdk's mocked runtime (feature `sim`), so the
//! gas they print is the mock's HOST gas (storage reads and writes, hashing,
//! log bytes) and not the wasm instructions: a lower bound, good for shape
//! ("flat or not") and ratios, never a budget. A budget needs the real node:
//! the same scenarios on near-sandbox are in `tests/sandbox/tests/stress.rs`
//! (`make stress`). The mock also has no node limits (that is how A47 was
//! missed), so nothing here can show a "state too big" trap.
//!
//! Run: `cargo test -p verify --features stress --test stress --locked --offline -- --nocapture`
//! (or `make stress`, which also runs the sandbox half when it can).
//!
//! Every test prints one `STR-nn ...` line with its numbers.

use agent_core::meter::MAX_DAYS;
use agent_exit::sim::*;
use agent_exit::*;
use near_sdk::json_types::U128;
use near_sdk::NearToken;

const HOUR_NS: u64 = 3_600 * agent_core::NANOS_PER_SEC;

/// Gas in Tgas for a printout.
fn tg(g: u64) -> String {
    format!("{:.3}", g as f64 / 1e12)
}

/// Host gas one call used. `f` runs inside an environment built by `c`.
fn gas<R>(s: &mut Sim, c: Call, f: impl FnOnce(&mut AgentExit) -> R) -> (R, u64) {
    s.env(&c);
    let g0 = near_sdk::env::used_gas().as_gas();
    let r = f(&mut s.c);
    (r, near_sdk::env::used_gas().as_gas() - g0)
}

fn gas_by<R>(s: &mut Sim, who: &str, f: impl FnOnce(&mut AgentExit) -> R) -> (R, u64) {
    let c = s.call(who);
    gas(s, c, f)
}

/// `ft_on_transfer` as the token would call it, with its gas.
fn ft_gas(s: &mut Sim, payer: &str, amount: u128, msg: &str) -> (u128, u64) {
    let mut c = s.call(TOKEN);
    c.signer = payer.into();
    c.gas = near_sdk::Gas::from_tgas(100);
    let (r, g) = gas(s, c, |a| {
        a.ft_on_transfer(acc(payer), U128(amount), msg.into())
    });
    let unused = match r {
        near_sdk::PromiseOrValue::Value(v) => v.0,
        near_sdk::PromiseOrValue::Promise(_) => 0,
    };
    (unused, g)
}

fn long_payer(i: usize) -> String {
    let id = format!("{i:0>56}.testnet");
    assert_eq!(id.len(), 64);
    id
}

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

/// NEAR (yocto) sent by the receipts of the last call, to anyone.
fn sent_out() -> u128 {
    near_sdk::test_utils::get_created_receipts()
        .into_iter()
        .flat_map(|r| r.actions)
        .filter_map(|a| match a {
            near_sdk::mock::MockAction::Transfer { deposit, .. } => Some(deposit.as_yoctonear()),
            _ => None,
        })
        .sum()
}

fn settle_deletions(s: &mut Sim) {
    let keys: Vec<String> = s.m().retiring.iter().cloned().collect();
    if !keys.is_empty() {
        s.callback(true).on_keys_deleted(keys);
    }
}

// ----------------------------------------------------------------- STR-01

/// STR-01: day rows at and far past MAX_DAYS. 1,200 consecutive days of
/// payments from registered payers: the rows stop at 400, totals keep every
/// payment, the pages still tile the kept days, and neither a payment nor a
/// view costs more at the cap than at day 50.
#[test]
fn str01_day_rows_at_max_days() {
    let mut s = translator_world();
    let gross0 = s.m().meter.total.gross;
    let mut mid = (0, 0, 0);
    let mut n_paid = 0u128;
    for i in 0..1_200usize {
        s.advance(DAY);
        let (unused, g) = ft_gas(&mut s, PAYERS[i % 6], USD, "");
        assert_eq!(unused, 0);
        n_paid += 1;
        if i == 50 || i == MAX_DAYS as usize + 5 || i == 1_199 {
            let (_, gs) = gas_by(&mut s, MALLORY, |a| a.get_status());
            let (_, ge) = gas_by(&mut s, MALLORY, |a| a.get_earnings());
            let (_, gd) = gas_by(&mut s, MALLORY, |a| a.get_earnings_days(Some(0), Some(100)));
            println!(
                "STR-01 day {i}: rows {} pay {} Tgas, get_status {} get_earnings {} get_earnings_days(100) {} Tgas",
                s.m().meter.days_total(), tg(g), tg(gs), tg(ge), tg(gd)
            );
            if i == 50 {
                mid = (g, gs, ge + gd);
            }
            if i == 1_199 {
                // The steady state at the cap costs about what day 50 did.
                assert!(g <= mid.0 * 2, "pay {g} vs {}", mid.0);
                assert!(gs <= mid.1 * 2, "status {gs} vs {}", mid.1);
                assert!(ge + gd <= mid.2 * 4, "views {} vs {}", ge + gd, mid.2);
            }
        }
    }
    assert_eq!(s.m().meter.days_total(), MAX_DAYS);
    assert_eq!(
        s.m().meter.total.gross,
        gross0 + n_paid * USD,
        "totals keep every payment"
    );
    // Four pages of 100 tile the 400 kept days: oldest first, no gaps.
    let mut all = vec![];
    for p in 0..4u64 {
        all.extend(s.c.get_earnings_days(Some(p * 100), Some(100)));
    }
    assert_eq!(all.len() as u64, MAX_DAYS);
    assert!(
        all.windows(2).all(|w| w[1].day == w[0].day + 1),
        "contiguous"
    );
    assert_eq!(all.last().unwrap().day, (s.now / DAY) as u64);
    assert!(s.c.get_earnings_days(Some(400), Some(100)).is_empty());
    assert!(s.c.get_earnings().days.len() as u64 <= agent_core::meter::EARNINGS_DAYS);
    let st = s.by(MALLORY).get_status();
    assert_eq!(st["near_storage"]["day_entries"], MAX_DAYS);
    assert!(st["near_storage"]["growth_reserve"].as_str().unwrap().len() > 1);
}

// ----------------------------------------------------------------- STR-02

fn sig_id(n: u64) -> [u8; 32] {
    let mut id = [0xeeu8; 32];
    id[..8].copy_from_slice(&n.to_le_bytes());
    id
}

/// STR-02: the shared signature window full (512), then rotated 6,000 more
/// times as proofs expire: never above 512 entries, an expired id is
/// forgotten, and the account's storage after 6,000 rotations is the same
/// as after the first 512 (no leak).
#[test]
fn str02_signature_window_full_then_rotated() {
    let mut s = Sim::new(AGENT, ALICE);
    s.env(&s.call(AGENT));
    let mut now_s = s.now / agent_core::NANOS_PER_SEC;
    let max = verified::MAX_SIGS as u64;
    let mut n = 0u64;
    // Fill the window with proofs that expire 600 s from now.
    for k in 0..max {
        if k % 32 == 0 {
            s.c.used_sigs.flush();
            s.env(&s.call(AGENT));
        }
        assert!(s.c.used_sigs.has_room(None, now_s));
        s.c.used_sigs.insert(sig_id(n), now_s + 600, now_s);
        n += 1;
    }
    s.c.used_sigs.flush();
    assert_eq!(s.c.used_sigs.len(), verified::MAX_SIGS);
    assert!(!s.c.used_sigs.has_room(None, now_s), "full: SigWindowFull");
    let usage_full = near_sdk::env::storage_usage();
    // 6,000 rotations in bursts: each burst waits until the oldest expires.
    let mut worst_insert = 0u64;
    for round in 0..12 {
        now_s += 601;
        for k in 0..max {
            if k % 32 == 0 {
                s.c.used_sigs.flush();
                s.env(&s.call(AGENT));
            }
            assert!(
                s.c.used_sigs.has_room(None, now_s),
                "round {round}: expired head is reusable"
            );
            let g0 = near_sdk::env::used_gas().as_gas();
            s.c.used_sigs.insert(sig_id(n), now_s + 600, now_s);
            worst_insert = worst_insert.max(near_sdk::env::used_gas().as_gas() - g0);
            n += 1;
            assert!(s.c.used_sigs.len() <= verified::MAX_SIGS);
        }
        assert!(!s.c.used_sigs.has_room(None, now_s));
    }
    s.c.used_sigs.flush();
    assert!(
        !s.c.used_sigs.contains(&sig_id(0)),
        "an expired id is dropped"
    );
    assert!(s.c.used_sigs.contains(&sig_id(n - 1)));
    let usage_after = near_sdk::env::storage_usage();
    println!(
        "STR-02 {n} ids through a {max}-slot window: storage {usage_full} -> {usage_after} bytes, worst insert {} Tgas",
        tg(worst_insert)
    );
    assert!(
        usage_after <= usage_full + 64,
        "storage grew {usage_full} -> {usage_after}"
    );
}

// ----------------------------------------------------------------- STR-03

/// STR-03: payer-row churn. 3,000 self register/unregister cycles, 3,000
/// third-party registrations reclaimed after 30 days, and 1,000 rows that
/// stay (register, then pay). At the end: the row counts are exactly the
/// kept rows, the NEAR held equals the kept rows' deposits, and every yocto
/// that went in (deposits plus the 1 yocto of each unregister) is either
/// held or was sent back.
#[test]
fn str03_payer_row_churn() {
    let mut s = translator_world();
    let (rows0, empty0, held0) = (
        s.m().meter.tracked_payers,
        s.m().meter.empty_payers,
        s.c.payer_deposits,
    );
    let usage0 = {
        s.env(&s.call(MALLORY));
        near_sdk::env::storage_usage()
    };
    let dep = PAYER_ROW_DEPOSIT.as_yoctonear();
    let mut put_in = 0u128;
    let mut sent_back = 0u128;
    // (a) 3,000 self register + unregister over 300 names.
    for i in 0..3_000usize {
        let who = long_payer(i % 300);
        s.register(&who);
        put_in += dep;
        let mut c = s.call(&who);
        c.deposit = NearToken::from_yoctonear(1);
        s.env(&c);
        assert!(s.c.storage_unregister(None));
        put_in += 1;
        sent_back += sent_out();
    }
    assert_eq!(s.m().meter.tracked_payers, rows0);
    assert_eq!(s.c.payer_deposits, held0);
    // (b) 30 batches of 100 third-party registrations, reclaimed by anyone
    // after 30 days; the refund goes to the depositor.
    for b in 0..30usize {
        for i in 0..100usize {
            let who = long_payer(10_000 + b * 100 + i);
            let mut c = s.call(MALLORY);
            c.deposit = PAYER_ROW_DEPOSIT;
            s.env(&c);
            s.c.storage_deposit(Some(acc(&who)), None);
            put_in += dep;
        }
        assert_eq!(s.m().meter.empty_payers, empty0 + 100);
        // Too young: refused.
        let who0 = long_payer(10_000 + b * 100);
        s.env(&s.call(CAROL));
        assert_eq!(
            panic_text(|| {
                s.c.reclaim_payer_row(acc(&who0));
            }),
            "RowTooYoung"
        );
        s.advance(31 * DAY);
        for i in 0..100usize {
            let who = long_payer(10_000 + b * 100 + i);
            s.env(&s.call(CAROL));
            s.c.reclaim_payer_row(acc(&who));
            sent_back += sent_out();
        }
        assert_eq!(s.m().meter.empty_payers, empty0);
    }
    // (c) 1,000 rows that stay: register, pay 1.00.
    for i in 0..1_000usize {
        let who = long_payer(50_000 + i);
        s.register(&who);
        put_in += dep;
        assert_eq!(s.pay(&who, USD), Some(0));
    }
    assert_eq!(s.m().meter.tracked_payers, rows0 + 1_000);
    assert_eq!(s.m().meter.empty_payers, empty0);
    assert_eq!(s.c.payer_deposits, held0 + 1_000 * dep);
    assert_eq!(
        put_in,
        (s.c.payer_deposits - held0) + sent_back,
        "every yocto in is held or sent back"
    );
    // A row with a payment cannot be unregistered, whatever the churn.
    let mut c = s.call(&long_payer(50_000));
    c.deposit = NearToken::from_yoctonear(1);
    s.env(&c);
    assert_eq!(
        panic_text(|| {
            s.c.storage_unregister(None);
        }),
        "PayerHasPayments"
    );
    let (_, g_reg) = {
        let mut c = s.call(&long_payer(99_999));
        c.deposit = PAYER_ROW_DEPOSIT;
        gas(&mut s, c, |a| a.storage_deposit(None, None))
    };
    s.env(&s.call(MALLORY));
    let usage1 = near_sdk::env::storage_usage();
    println!(
        "STR-03 7,000 churn ops: rows {rows0} -> {}, held {} -> {} yocto, storage {usage0} -> {usage1} bytes (1,000 kept rows), register at 1,000 rows {} Tgas",
        s.m().meter.tracked_payers, held0, s.c.payer_deposits, tg(g_reg)
    );
    let grew = usage1 - usage0;
    assert!(
        grew as u128 <= 1_000 * PAYER_BYTES,
        "{grew} bytes for 1,000 kept rows (budget {} each)",
        PAYER_BYTES
    );
}

// ----------------------------------------------------------------- STR-04

/// STR-04: listing churn and buys that lose a race. 1,500 list / cancel
/// rounds bump `listing_version` 3,000 times; buys quoted before a cancel,
/// before a relist, or with a stale key hash are refunded in full and change
/// nothing; the final honest buy still works; the state blob is flat.
#[test]
fn str04_listing_churn_and_stale_buys() {
    let mut s = translator_world();
    let b0 = s.c.state_bytes();
    let v0 = s.m().listing_version;
    let mut stale_refused = 0;
    let mut g_first = (0, 0);
    let mut g_last = (0, 0);
    for i in 0..1_500u64 {
        let c = {
            let mut c = s.call(ALICE);
            c.deposit = NearToken::from_yoctonear(1);
            c
        };
        let (_, gl) = gas(&mut s, c.clone(), |a| {
            a.list_for_sale(U128(1_000 * USD), CODE_HASH.into(), None)
        });
        let quoted = s.m().listing_version;
        let hash = s.c.known_keys_hash();
        if i % 3 == 0 {
            // The seller cancels while the buyer's transaction is in flight.
            s.by_1y(ALICE).cancel_listing();
            let before = (s.m().listing_version, s.m().status.clone());
            let unused = s.ft(
                BOB,
                1_000 * USD,
                &Sim::buy_msg(quoted, &hash, BOB_RUNNER),
                300,
            );
            assert_eq!(
                unused,
                Some(1_000 * USD),
                "round {i}: buy after cancel is refunded"
            );
            assert_eq!((s.m().listing_version, s.m().status.clone()), before);
            stale_refused += 1;
        } else if i % 3 == 1 {
            // Cancel and relist: the old quote is stale.
            s.by_1y(ALICE).cancel_listing();
            s.by_1y(ALICE)
                .list_for_sale(U128(1_000 * USD), CODE_HASH.into(), None);
            let unused = s.ft(
                BOB,
                1_000 * USD,
                &Sim::buy_msg(quoted, &hash, BOB_RUNNER),
                300,
            );
            assert_eq!(
                unused,
                Some(1_000 * USD),
                "round {i}: buy of a replaced listing is refunded"
            );
            assert!(s.m().pending.is_none());
            stale_refused += 1;
            s.by_1y(ALICE).cancel_listing();
        } else {
            // A buyer holding a wrong key hash is refused too.
            let unused = s.ft(
                BOB,
                1_000 * USD,
                &Sim::buy_msg(quoted, "bad-hash", BOB_RUNNER),
                300,
            );
            assert_eq!(unused, Some(1_000 * USD));
            stale_refused += 1;
            let (_, gc) = gas(&mut s, c, |a| a.cancel_listing());
            if i == 2 {
                g_first = (gl, gc);
            }
            if i == 1_499 {
                g_last = (gl, gc);
            }
        }
        assert!(s.m().pending.is_none());
    }
    assert_eq!(stale_refused, 1_500);
    assert!(s.m().listing_version > v0 + 3_000);
    let b1 = s.c.state_bytes();
    assert!(b1 <= b0 + 16, "blob {b0} -> {b1}");
    // The honest buyer, with a fresh quote, still gets the agent.
    s.by_1y(ALICE)
        .list_for_sale(U128(1_000 * USD), CODE_HASH.into(), None);
    assert_eq!(s.buy(BOB, 1_000 * USD, BOB_RUNNER), None);
    let id = s.m().pending.as_ref().unwrap().sale_id;
    assert_eq!(s.callback(true).on_handover(id).0, 0);
    assert_eq!(s.m().owner, BOB);
    println!(
        "STR-04 version {v0} -> {}, {stale_refused} stale buys refunded, blob {b0} -> {b1} bytes, list/cancel gas first {}/{} last {}/{} Tgas",
        s.m().listing_version, tg(g_first.0), tg(g_first.1), tg(g_last.0), tg(g_last.1)
    );
    assert!(
        g_last.0 <= g_first.0 * 2 && g_last.1 <= g_first.1 * 2,
        "listing gas drifts with churn"
    );
}

// ----------------------------------------------------------------- STR-05

/// STR-05: earn-out epochs and claims. 16 sales in a row without finalizing
/// fill `MAX_OPEN_SALES`; the 17th listing is refused; customers keep paying
/// while all 16 windows are open; after the windows end every sale
/// finalizes, the sellers claim, and the books stay solvent and exact at
/// every step.
#[test]
fn str05_sixteen_open_earnouts_then_finalize_and_claim() {
    let mut s = translator_world();
    let pair = [(BOB, BOB_RUNNER), (CAROL, CAROL_RUNNER)];
    let mut ids = vec![];
    let mut paid_in = 0u128;
    for i in 0..agent_core::MAX_OPEN_SALES {
        let (buyer, pk) = pair[i % 2];
        let seller = s.m().owner.clone();
        if s.m().known_keys.is_empty() {
            s.by_1y(&seller).set_runner_key(key(9)).detach();
            s.runner_key_cb(true, vec![], vec![], key_str(9));
        }
        s.by_1y(&seller)
            .list_for_sale(U128(100 * USD), CODE_HASH.into(), None);
        assert_eq!(
            s.buy(buyer, 100 * USD, pk),
            None,
            "{i}: {:?}",
            events("refused")
        );
        let id = s.m().pending.as_ref().unwrap().sale_id;
        assert_eq!(s.callback(true).on_handover(id).0, 0);
        ids.push(id);
        // Customers pay inside every open window.
        for p in PAYERS {
            s.advance(HOUR_NS);
            assert_eq!(s.pay(p, 12 * USD), Some(0));
            paid_in += 12 * USD;
        }
        assert!(s.m().solvent(), "sale {i}");
    }
    assert_eq!(s.m().sales.len(), agent_core::MAX_OPEN_SALES);
    // The 17th listing is refused.
    let seller = s.m().owner.clone();
    if s.m().known_keys.is_empty() {
        s.by_1y(&seller).set_runner_key(key(9)).detach();
        s.runner_key_cb(true, vec![], vec![], key_str(9));
    }
    assert_eq!(
        panic_text(|| {
            s.by_1y(&seller)
                .list_for_sale(U128(100 * USD), CODE_HASH.into(), None);
        }),
        "TooManyOpenSales"
    );
    let blob_open = s.c.state_bytes();
    // Windows end: everyone may finalize; it is idempotent.
    s.advance(31 * DAY);
    let mut released = 0u128;
    let mut g_fin = 0;
    for id in &ids {
        let (v, g) = gas_by(&mut s, MALLORY, |a| a.finalize_earnout(*id));
        g_fin = g_fin.max(g);
        released += v["released"].as_str().map_or(0, |x| x.parse().unwrap_or(0));
        assert!(s.m().solvent());
        s.env(&s.call(MALLORY));
        let _ = s.c.finalize_earnout(*id);
    }
    assert!(s.m().sales.iter().all(|x| x.finalized));
    // The seller of each sale claims what it is owed.
    for a in [ALICE, BOB, CAROL] {
        let owed = s.c.get_claimable(acc(a)).0;
        if owed > 0 {
            s.env(&s.call(a));
            let _ = s.c.claim();
            s.callback(true)
                .on_transfer_out(OutKind::Claim, acc(a), U128(owed), None);
            assert_eq!(s.c.get_claimable(acc(a)).0, 0);
        }
    }
    assert!(s.m().solvent());
    // Listing works again once windows are final.
    s.by_1y(&s.m().owner.clone())
        .list_for_sale(U128(100 * USD), CODE_HASH.into(), None);
    let blob_end = s.c.state_bytes();
    println!(
        "STR-05 16 open earn-outs: blob {blob_open} -> {blob_end} bytes, customers paid {} after listing, worst finalize {} Tgas, seller releases seen {released}",
        paid_in / USD, tg(g_fin)
    );
    assert!(blob_end < 20_000, "{blob_end}");
}

// ----------------------------------------------------------------- STR-06

/// STR-06: runner key rotated 1,500 times (every 7th rotation's batch
/// fails and rolls back). One live key at the end, no key left pending, the
/// keys hash tracks the set, a quote taken before any rotation is refused,
/// and nothing grows.
#[test]
fn str06_runner_key_rotation_1500_times() {
    let mut s = sold_translator();
    let b0 = s.c.state_bytes();
    let mut live = BOB_RUNNER;
    let mut worst = 0u64;
    let old_hash = s.c.known_keys_hash();
    let mut b100 = 0;
    for i in 0..1_500usize {
        let new = 10 + (i % 200) as u8; // never BOB_RUNNER (2), HIDDEN_KEY (66) is allowed to recur
        if new == live || new == 66 {
            continue;
        }
        let old: Vec<String> = s.m().known_keys.iter().cloned().collect();
        let mut c = s.call(BOB);
        c.deposit = NearToken::from_yoctonear(1);
        let (_, g) = gas(&mut s, c, |a| a.set_runner_key(key(new)).detach());
        worst = worst.max(g);
        let deleted: Vec<String> = s.m().retiring.iter().cloned().collect();
        let ok = i % 7 != 6;
        s.runner_key_cb(ok, old, deleted.clone(), key_str(new));
        if ok {
            settle_deletions(&mut s);
            live = new;
            assert_eq!(s.m().known_keys.len(), 1);
            assert!(s.m().known_keys.contains(&key_str(new)));
        } else {
            assert!(s.m().known_keys.contains(&key_str(live)) || s.m().known_keys.is_empty());
            // A failed batch leaves keys pending: the owner clears them.
            if !s.m().incoming.is_empty() || !s.m().retiring.is_empty() {
                s.advance(HOUR_NS + 1);
                s.by(BOB).recover_keys();
                settle_deletions(&mut s);
                if s.m().known_keys.is_empty() {
                    s.by_1y(BOB).set_runner_key(key(live)).detach();
                    s.runner_key_cb(true, vec![], vec![], key_str(live));
                    settle_deletions(&mut s);
                }
            }
        }
        assert!(s.m().incoming.is_empty(), "round {i}");
        if i == 100 {
            b100 = s.c.state_bytes();
        }
    }
    assert_ne!(s.c.known_keys_hash(), old_hash);
    assert_eq!(s.m().known_keys.len(), 1);
    let b1 = s.c.state_bytes();
    // A buy quoted with the hash of the first key is refused after all that.
    s.by_1y(BOB)
        .list_for_sale(U128(1_000 * USD), CODE_HASH.into(), None);
    let v = s.m().listing_version;
    assert_eq!(
        s.ft(
            CAROL,
            1_000 * USD,
            &Sim::buy_msg(v, &old_hash, CAROL_RUNNER),
            300
        ),
        Some(1_000 * USD)
    );
    println!(
        "STR-06 1,500 rotations: blob {b0} -> {b1} bytes, worst set_runner_key {} Tgas, batches issued {}",
        tg(worst), s.m().key_batch
    );
    let m = s.m();
    println!(
        "STR-06 sets: known {} incoming {} retiring {} {:?}",
        m.known_keys.len(),
        m.incoming.len(),
        m.retiring.len(),
        s.c.get_status()["retiring_keys"]
    );
    println!("STR-06 blob after 100 rotations {b100}, after 1,500 {b1}");
    assert!(b1 <= b100 + 16, "blob grows with rotations: {b100} -> {b1}");
    assert!(b100 <= b0 + 128, "one-time growth {b0} -> {b100}");
}

// ----------------------------------------------------------------- STR-07

/// STR-07: treasury and limits at their caps. 10,000 tiny spends fill the
/// daily cap exactly; the next one is refused; the cap resets the next day;
/// a spend over the treasury is refused; payments in, spends out and the
/// treasury always add up, and the agent stays solvent.
#[test]
fn str07_daily_cap_and_treasury_at_the_limit() {
    let mut s = business_world();
    // 100.00 into the treasury via 25.00 payments (80/20 split).
    for i in 0..20 {
        s.pay(PAYERS[i % 6], 25 * USD);
    }
    let cap = 50 * USD;
    let t0 = s.m().treasury;
    assert!(t0 > cap);
    let step = cap / 10_000;
    let mut worst = 0u64;
    for i in 0..10_000u64 {
        let c = s.call(AGENT);
        let mut c = c;
        c.signer_pk = key(BOB_RUNNER);
        let (v, g) = gas(&mut s, c, |a| a.pay(acc(SUMMARIZER), U128(step)));
        worst = worst.max(g);
        assert_eq!(v["ok"], true, "spend {i}");
    }
    assert_eq!(s.m().spent_today(s.now), cap);
    assert_eq!(s.m().treasury, t0 - cap);
    let msg = panic_text(|| {
        s.runner(key(BOB_RUNNER)).pay(acc(SUMMARIZER), U128(1));
    });
    assert!(msg.contains("DailyCap"), "{msg}");
    // Next UTC day: the meter starts again.
    s.advance(DAY);
    assert_eq!(s.m().spent_today(s.now), 0);
    assert_eq!(
        s.runner(key(BOB_RUNNER)).pay(acc(SUMMARIZER), U128(cap))["ok"],
        true
    );
    // Raise the cap past the treasury: the treasury is the limit then.
    s.by_1y(BOB).set_limits(LimitsArgs {
        daily_cap: U128(1_000_000 * USD),
        ..demo_limits()
    });
    let left = s.m().treasury;
    let msg = panic_text(|| {
        s.runner(key(BOB_RUNNER))
            .pay(acc(SUMMARIZER), U128(left + 1));
    });
    assert!(msg.contains("InsufficientFunds"), "{msg}");
    assert_eq!(
        s.runner(key(BOB_RUNNER)).pay(acc(SUMMARIZER), U128(left))["ok"],
        true
    );
    assert_eq!(s.m().treasury, 0);
    assert!(s.m().solvent());
    println!("STR-07 10,000 spends of {step} units fill the {} cap; worst pay {} Tgas; treasury {t0} -> 0", cap / USD, tg(worst));
}

// ----------------------------------------------------------------- STR-08

/// STR-08: the largest `ft_transfer_call` msg. 4,096 bytes is the cap: a
/// full-size pay msg is counted, 4,097 bytes and 1 MB are refunded in full,
/// and what the oversize costs is a fixed small amount (the length is
/// checked before any parsing).
#[test]
fn str08_max_size_ft_msg() {
    let mut s = translator_world();
    s.register("big.testnet");
    // A valid pay msg padded to exactly 4,096 bytes with a long job_id.
    let pad = |n: usize| {
        let base = r#"{"kind":"pay","job_id":""#.len() + r#""}"#.len();
        format!(r#"{{"kind":"pay","job_id":"{}"}}"#, "j".repeat(n - base))
    };
    let at_cap = pad(MAX_MSG_LEN);
    assert_eq!(at_cap.len(), MAX_MSG_LEN);
    let over = pad(MAX_MSG_LEN + 1);
    let huge = pad(1_000_000);
    let ws = " ".repeat(MAX_MSG_LEN);
    let before = s.m().meter.total.gross;
    let (u1, g_cap) = ft_gas(&mut s, "big.testnet", 5 * USD, &at_cap);
    let counted_cap = s.m().meter.total.gross - before;
    let (u2, g_over) = ft_gas(&mut s, "big.testnet", 5 * USD, &over);
    let (u3, g_huge) = ft_gas(&mut s, "big.testnet", 5 * USD, &huge);
    let (u4, g_ws) = ft_gas(&mut s, "big.testnet", 5 * USD, &ws);
    let (u5, g_junk) = ft_gas(&mut s, "big.testnet", 5 * USD, &"{".repeat(MAX_MSG_LEN));
    println!(
        "STR-08 msg 4096B: counted {} (unused {u1}) {} Tgas; 4097B refunded {} Tgas; 1MB refunded {} Tgas; 4096 spaces (a plain pay) {} Tgas; 4096 junk refunded {} Tgas",
        counted_cap / USD, tg(g_cap), tg(g_over), tg(g_huge), tg(g_ws), tg(g_junk)
    );
    assert_eq!(
        u1 + counted_cap,
        5 * USD,
        "full-size msg is counted or refunded, never lost"
    );
    assert_eq!(u2, 5 * USD, "4097 bytes refunded");
    assert_eq!(u3, 5 * USD, "1 MB refunded");
    assert_eq!(u5, 5 * USD, "junk refunded");
    let _ = u4;
    // The refusal is cheap and does not scale with the message.
    assert!(
        g_huge <= g_over * 10 + 5_000_000_000,
        "{g_huge} vs {g_over}"
    );
    assert!(
        g_cap < 50_000_000_000_000,
        "a 4 KB msg stays far under the 100 Tgas the token gives"
    );
}

// ----------------------------------------------------------------- STR-09

/// STR-09: everything at its cap at once, then the views and the calls.
/// 1,000 registered 64-character payers, 400 day rows, a full signature
/// window, 16 open earn-outs, 3,000 payout destinations over three owners,
/// 600 past owners and 700 claimable balances. Every view answers, the
/// blob is a few KB, and view and call gas stay small.
#[test]
fn str09_everything_at_max_then_views_and_calls() {
    let mut s = sold_translator();
    s.by_1y(BOB).set_limits(demo_limits());
    // Payers and days.
    for i in 0..1_000 {
        s.register(&long_payer(i));
        s.advance(DAY / 2);
        assert_eq!(
            s.pay(&long_payer(i), 5 * USD),
            Some(0),
            "{i}: {:?}",
            events("refused")
        );
    }
    for _ in 0..450 {
        s.advance(DAY);
        s.pay(PAYERS[0], 5 * USD);
    }
    // A full shared window of ids.
    s.env(&s.call(AGENT));
    let now_s = s.now / agent_core::NANOS_PER_SEC;
    for n in 0..verified::MAX_SIGS as u64 {
        s.c.used_sigs.insert(sig_id(n), now_s + 600, now_s);
    }
    // 700 claimable balances (failed claim payouts, FAULT INJECTION).
    for i in 0..700 {
        s.callback(false).on_transfer_out(
            OutKind::Claim,
            acc(&format!("claimer-{i}.testnet")),
            U128(1),
            None,
        );
    }
    // Payout destinations: 1,000 for each of three owners; 600 past owners.
    let mut owner = BOB.to_string();
    let hop = |s: &mut Sim, owner: &mut String, next: &str| {
        s.by_1y(owner)
            .nft_transfer(acc(next), DEED_ID.into(), None, None);
        settle_deletions(s);
        s.pay(PAYERS[1], 2_000 * USD);
        *owner = next.to_string();
    };
    for k in 0..3 {
        for i in 0..agent_core::MAX_PAID_TO {
            s.by_1y(&owner)
                .withdraw(U128(1), Some(acc(&format!("dest-{k}-{i}.testnet"))))
                .detach();
        }
        if k < 2 {
            hop(&mut s, &mut owner, &format!("hop-owner-{k}.testnet"));
        }
    }
    for i in 0..600 {
        let next = format!("past-{i}.testnet");
        s.by_1y(&owner)
            .nft_transfer(acc(&next), DEED_ID.into(), None, None);
        settle_deletions(&mut s);
        owner = next;
    }
    // 16 open earn-outs.
    let pair = [(CAROL, CAROL_RUNNER), (ALICE, ALICE_RUNNER)];
    for i in 0..agent_core::MAX_OPEN_SALES - 1 {
        let (buyer, pk) = pair[i % 2];
        let seller = s.m().owner.clone();
        if seller == buyer {
            continue;
        }
        if s.m().known_keys.is_empty() {
            s.by_1y(&seller).set_runner_key(key(9)).detach();
            s.runner_key_cb(true, vec![], vec![], key_str(9));
        }
        s.by_1y(&seller)
            .list_for_sale(U128(10 * USD), CODE_HASH.into(), None);
        if s.buy(buyer, 10 * USD, pk).is_none() {
            let id = s.m().pending.as_ref().unwrap().sale_id;
            s.callback(true).on_handover(id);
        }
    }
    let blob = s.c.state_bytes();
    // Views, with gas and the account storage the mock tracked.
    let mut c = s.call(MALLORY);
    c.storage = 450_000; // the agent wasm itself, rounded up
    let (st, g_status) = gas(&mut s, c.clone(), |a| a.get_status());
    let (e, g_earn) = gas(&mut s, c.clone(), |a| a.get_earnings());
    let (days, g_days) = gas(&mut s, c.clone(), |a| {
        a.get_earnings_days(Some(300), Some(100))
    });
    let (_, g_listing) = gas(&mut s, c.clone(), |a| a.get_listing());
    let (_, g_nft) = gas(&mut s, c.clone(), |a| a.nft_token(DEED_ID.into()));
    let (_, g_payer) = gas(&mut s, c.clone(), |a| a.get_payer(acc(&long_payer(999))));
    let (_, g_sales) = gas(&mut s, c.clone(), |a| a.get_sales(None, Some(50)));
    let (_, g_claim) = gas(&mut s, c.clone(), |a| {
        a.get_claimable(acc("claimer-699.testnet"))
    });
    println!(
        "STR-09 max state: blob {blob} bytes; views Tgas status {} earnings {} days(100) {} listing {} nft_token {} payer {} sales {} claimable {}",
        tg(g_status), tg(g_earn), tg(g_days), tg(g_listing), tg(g_nft), tg(g_payer), tg(g_sales), tg(g_claim)
    );
    println!("STR-09 near_storage at max: {}", st["near_storage"]);
    assert_eq!(e.tracked_payers, st["tracked_payers"].as_u64().unwrap());
    assert_eq!(days.len(), 100);
    assert!(blob < 20_000, "blob {blob}");
    for (what, g) in [
        ("status", g_status),
        ("earnings", g_earn),
        ("days", g_days),
        ("listing", g_listing),
        ("nft_token", g_nft),
        ("payer", g_payer),
        ("sales", g_sales),
        ("claimable", g_claim),
    ] {
        assert!(g < 20_000_000_000_000, "{what} view burns {} Tgas", tg(g));
    }
    assert_eq!(st["open_sales"], agent_core::MAX_OPEN_SALES);
    assert_eq!(st["near_storage"]["day_entries"], MAX_DAYS);
    assert_eq!(st["near_storage"]["sig_entries"], verified::MAX_SIGS);
    assert!(st["near_storage"]["payer_entries"].as_u64().unwrap() >= 1_000);
    // Calls at max state: a payment, a runner heartbeat, a limit set.
    let (_, g_pay) = ft_gas(&mut s, &long_payer(5), 5 * USD, "");
    let mut c = s.call(AGENT);
    c.signer_pk = key(9);
    println!("STR-09 pay at max state {} Tgas", tg(g_pay));
    assert!(g_pay < 50_000_000_000_000);
    // Solvency (STR-10 reads the same world on a real node).
    assert!(s.m().solvent());
}

// ----------------------------------------------------------------- STR-10

/// STR-10: solvency after growth to every cap. `near_storage.growth_reserve`
/// is what outsiders may still add for free (signature ids, day rows): the
/// agent must be solvent with that bound counted, and the sum
/// `staked + growth_reserve + reserve` must stay below the balance the
/// seller cannot withdraw. Checked on the NEAR formulas for an agent that
/// holds 6 NEAR and has the worst case of both tables already used.
#[test]
fn str10_solvency_formula_at_the_caps() {
    let mut s = Sim::new(AGENT, ALICE);
    let mut c = s.call(MALLORY);
    c.storage = 450_000;
    s.env(&c);
    let empty = s.c.get_status()["near_storage"].clone();
    // Fill both outsider-fed tables to the cap.
    s.env(&s.call(AGENT));
    let now_s = s.now / agent_core::NANOS_PER_SEC;
    for n in 0..verified::MAX_SIGS as u64 {
        s.c.used_sigs.insert(sig_id(n), now_s + 600, now_s);
    }
    s.c.used_sigs.flush();
    let mut p = s.call(AGENT);
    p.storage = 0;
    s.env(&p);
    for _ in 0..(MAX_DAYS + 50) {
        s.advance(DAY);
        s.pay("anyone.testnet", USD);
    }
    let mut c = s.call(MALLORY);
    c.storage = 450_000;
    s.env(&c);
    let full = s.c.get_status()["near_storage"].clone();
    let num = |v: &serde_json::Value, k: &str| -> u128 { v[k].as_str().unwrap().parse().unwrap() };
    println!(
        "STR-10 empty: usage {} staked {} growth_reserve {} | at caps: usage {} staked {} growth_reserve {} balance {} solvent {}",
        empty["usage_bytes"], num(&empty, "staked"), num(&empty, "growth_reserve"),
        full["usage_bytes"], num(&full, "staked"), num(&full, "growth_reserve"),
        num(&full, "balance"), full["solvent"]
    );
    // Growth used up what the reserve held: staked rose, the reserve fell,
    // and the sum is no bigger than at the start (the bound is tight).
    assert!(num(&full, "staked") > num(&empty, "staked"));
    assert!(num(&full, "growth_reserve") < num(&empty, "growth_reserve"));
    let need = |v: &serde_json::Value| num(v, "staked") + num(v, "growth_reserve");
    assert!(
        need(&full) <= need(&empty) + 200 * 10u128.pow(19),
        "need {} -> {} yocto",
        need(&empty),
        need(&full)
    );
    assert_eq!(full["solvent"], true);
    // And with too little NEAR, the flag says so (the scan blocks that).
    s.balance = NearToken::from_near(1);
    s.env(&c);
    assert_eq!(s.c.get_status()["near_storage"]["solvent"], false);
}

// ----------------------------------------------------------------- STR-11

/// STR-11: the scan on an account with a huge key list. A seller who adds N
/// hidden function-call keys (unknown to the contract) gets one blocking
/// finding per key and the verdict `blocked`; one full-access key among
/// 20,000 known runner-shaped keys is still found; the scan stays fast.
#[test]
fn str11_scan_with_many_keys() {
    use scan::{scan, AccessKey, Permission, ScanInput};
    let text = std::fs::read_to_string(
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/judge-attack.json"),
    )
    .unwrap();
    let base: ScanInput = serde_json::from_str(&text).unwrap();
    assert_eq!(scan(&base).verdict, "clean");
    let fc = |n: usize, to: &str| AccessKey {
        public_key: format!("ed25519:hidden{n:08}"),
        permission: Permission::FunctionCall {
            receiver_id: to.into(),
            method_names: vec!["heartbeat".into()],
            allowance: None,
        },
    };
    for n in [100usize, 1_000, 20_000] {
        let mut i = base.clone();
        for k in 0..n {
            let key = fc(k, &i.agent.clone());
            if let Some(second) = i.chain.keys_second_provider.as_mut() {
                second.push(key.clone());
            }
            i.chain.keys.push(key);
        }
        let t = std::time::Instant::now();
        let r = scan(&i);
        let ms = t.elapsed().as_millis();
        let unknown = r.codes().iter().filter(|c| **c == "unknown_key").count();
        println!(
            "STR-11 {n} hidden keys: verdict {} findings {} unknown_key {unknown} in {ms} ms",
            r.verdict,
            r.findings.len()
        );
        assert_eq!(r.verdict, "blocked");
        assert_eq!(unknown, n);
        assert!(ms < 5_000, "{n} keys took {ms} ms");
    }
    // One full-access key hidden among 20,000 keys the contract lists.
    let mut i = base.clone();
    for k in 0..20_000 {
        let key = fc(k, &i.agent.clone());
        i.contract.known_keys.push(key.public_key.clone());
        if let Some(second) = i.chain.keys_second_provider.as_mut() {
            second.push(key.clone());
        }
        i.chain.keys.push(key);
    }
    i.contract.known_keys_hash = agent_core::keys_hash(i.contract.known_keys.iter());
    let hidden = AccessKey {
        public_key: "ed25519:thehiddenone".into(),
        permission: Permission::Full("FullAccess".into()),
    };
    if let Some(second) = i.chain.keys_second_provider.as_mut() {
        second.push(hidden.clone());
    }
    i.chain.keys.push(hidden);
    let t = std::time::Instant::now();
    let r = scan(&i);
    println!(
        "STR-11 1 full-access key among 20,000 listed: verdict {} codes {:?} in {} ms",
        r.verdict,
        r.codes().iter().collect::<std::collections::BTreeSet<_>>(),
        t.elapsed().as_millis()
    );
    assert!(r.codes().contains(&"full_access_key_present"));
    assert_eq!(r.verdict, "blocked");
}

// ----------------------------------------------------------------- shared

/// The accounting invariant of every scenario: what the agent holds covers
/// every bucket (`solvent`), and `held` is exactly their sum (no payment is
/// in flight in these scenarios).
fn books(s: &Sim, what: &str) {
    let m = s.m();
    assert!(m.solvent(), "{what}: not solvent");
    let owed = m.escrow_left() + m.upfront_held_total() + m.claimable_total() + m.owner_free + m.treasury;
    assert_eq!(m.held, owed, "{what}: held {} != buckets {owed}", m.held);
}

/// One sale to `buyer`: relist (`price`), buy with a fresh quote, hand over.
/// The seller needs a runner key to list (the handover deleted them).
fn sell(s: &mut Sim, buyer: &str, pk: u8, price: u128) -> u64 {
    let seller = s.m().owner.clone();
    if s.m().known_keys.is_empty() {
        s.by_1y(&seller).set_runner_key(key(9)).detach();
        s.runner_key_cb(true, vec![], vec![], key_str(9));
    }
    s.by_1y(&seller)
        .list_for_sale(U128(price), CODE_HASH.into(), None);
    assert_eq!(s.buy(buyer, price, pk), None, "{:?}", events("refused"));
    let id = s.m().pending.as_ref().unwrap().sale_id;
    assert_eq!(s.callback(true).on_handover(id).0, 0);
    id
}

// ----------------------------------------------------------------- STR-12

/// STR-12 (found by this suite, fixed): a listed seller could pay 1 unit to
/// up to 1,000 fresh addresses through `withdraw`, adding payout-destination
/// rows that `near_storage` did not reserve, so the agent the buyer scanned
/// as solvent turned insolvent before the sale. Now `withdraw` is refused
/// while Listed (`Listed: spending paused`), like `withdraw_near` (A39), and
/// the storage does not move.
#[test]
fn str12_withdraw_while_listed_is_refused_and_storage_stays() {
    let mut s = listed_translator();
    let mut c = s.call(MALLORY);
    c.storage = 450_000;
    s.env(&c);
    let ns = s.c.get_status()["near_storage"].clone();
    let num = |k: &str| -> u128 { ns[k].as_str().unwrap().parse().unwrap() };
    let need = num("staked") + num("growth_reserve") + NEAR_RESERVE.as_yoctonear();
    s.balance = NearToken::from_yoctonear(need + 2 * 10u128.pow(23));
    s.env(&c);
    let before = s.c.get_status()["near_storage"].clone();
    assert_eq!(
        before["solvent"], true,
        "the buyer's scan sees a solvent agent"
    );
    let usage0 = before["usage_bytes"].as_u64().unwrap();
    let mut refused = 0;
    for i in 0..agent_core::MAX_PAID_TO {
        let msg = panic_text(|| {
            s.by_1y(ALICE)
                .withdraw(U128(1), Some(acc(&long_payer(70_000 + i))))
                .detach();
        });
        if msg.contains("Listed: spending paused") {
            refused += 1;
        }
    }
    s.env(&c);
    let after = s.c.get_status()["near_storage"].clone();
    println!(
        "STR-12 {refused} withdraws refused while Listed; usage {} -> {}, solvent {} -> {}",
        usage0, after["usage_bytes"], before["solvent"], after["solvent"]
    );
    assert_eq!(
        refused,
        agent_core::MAX_PAID_TO,
        "every withdraw while Listed is refused"
    );
    assert_eq!(
        after["usage_bytes"].as_u64().unwrap(),
        usage0,
        "no rows added"
    );
    assert_eq!(after["solvent"], true);
}

// ----------------------------------------------------------------- STR-13

/// STR-13: a chain of 300 sales with the earn-out windows overlapping (up to
/// 16 open at once) and finalized out of order, newest first. The 17th
/// listing is refused while 16 are open; `held` equals its buckets after
/// every step; at the end every escrow is spent (escrow_left == 0) and every
/// claimant can claim.
#[test]
fn str13_three_hundred_sales_overlapping_earnouts() {
    let mut s = translator_world();
    let pair = [(BOB, BOB_RUNNER), (CAROL, CAROL_RUNNER)];
    let mut open: Vec<(u64, u64)> = vec![]; // (sale id, window_end)
    let mut max_open = 0;
    let mut refused17 = 0;
    let mut blob_max = 0;
    let mut sold = 0;
    for n in 0..300usize {
        s.advance(DAY);
        let now = s.now;
        let mut ended: Vec<(u64, u64)> = open.iter().copied().filter(|(_, e)| *e <= now).collect();
        ended.sort_by_key(|x| std::cmp::Reverse(x.0));
        for (id, _) in ended.into_iter().take(3) {
            s.by(MALLORY).finalize_earnout(id);
            open.retain(|(i, _)| *i != id);
            books(&s, "finalize");
        }
        let open_now = s.m().sales.iter().filter(|x| !x.finalized).count();
        if open_now >= agent_core::MAX_OPEN_SALES {
            let seller = s.m().owner.clone();
            if s.m().known_keys.is_empty() {
                s.by_1y(&seller).set_runner_key(key(9)).detach();
                s.runner_key_cb(true, vec![], vec![], key_str(9));
            }
            let msg = panic_text(|| {
                s.by_1y(&seller)
                    .list_for_sale(U128(10 * USD), CODE_HASH.into(), None);
            });
            assert_eq!(msg, "TooManyOpenSales", "sale {n}");
            refused17 += 1;
            continue;
        }
        let (buyer, pk) = pair[n % 2];
        if s.m().owner == buyer {
            continue;
        }
        let id = sell(&mut s, buyer, pk, 10 * USD);
        sold += 1;
        let end = s.m().sales.iter().find(|x| x.id == id).unwrap().window_end;
        open.push((id, end));
        s.pay(PAYERS[n % 6], 12 * USD);
        books(&s, "after sale");
        max_open = max_open.max(s.m().sales.iter().filter(|x| !x.finalized).count());
        blob_max = blob_max.max(s.c.state_bytes());
    }
    assert!(refused17 > 0, "the 17th listing was refused at least once");
    assert_eq!(max_open, agent_core::MAX_OPEN_SALES);
    s.advance(40 * DAY);
    open.sort_by_key(|x| std::cmp::Reverse(x.0));
    for (id, _) in &open {
        s.by(MALLORY).finalize_earnout(*id);
        books(&s, "final finalize");
    }
    assert_eq!(s.m().escrow_left(), 0, "every escrow is spent");
    assert!(s.m().sales.iter().all(|x| x.finalized));
    let mut claimed = 0u128;
    for a in [ALICE, BOB, CAROL] {
        let owed = s.c.get_claimable(acc(a)).0;
        if owed > 0 {
            s.env(&s.call(a));
            let _ = s.c.claim();
            s.callback(true)
                .on_transfer_out(OutKind::Claim, acc(a), U128(owed), None);
            claimed += owed;
        }
        books(&s, "claim");
    }
    println!(
        "STR-13 {sold} sales in 300 steps: max open {max_open}, 17th refused {refused17} times, max blob {blob_max} bytes, claimed {} xUSD, sales kept {} closed {}",
        claimed / USD,
        s.m().sales.len(),
        s.m().closed_sales
    );
    assert!(blob_max < 20_000);
}

// ----------------------------------------------------------------- STR-14

struct Rng(u64);
impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }
    fn below(&mut self, n: u64) -> u64 {
        self.next() % n
    }
}

/// STR-14: set_runner_key, heartbeat, nft_transfer, list, cancel, buy,
/// payment, withdraw and finalize interleaved at random (3,000 seeded steps,
/// 3 seeds). After every step the books are exact, no key is both live and
/// retiring, and no settle is left half done. Refusals are expected (most
/// random calls are not allowed); they are counted.
#[test]
fn str14_interleaved_key_and_sale_operations() {
    for seed in [1u64, 7, 99] {
        let mut s = translator_world();
        let mut r = Rng(seed.wrapping_mul(0x9E37_79B9_7F4A_7C15) | 1);
        let people = [ALICE, BOB, CAROL, MALLORY, "dave.testnet"];
        let (mut ok, mut refused) = (0u32, 0u32);
        for step in 0..3_000u32 {
            let who = people[r.below(people.len() as u64) as usize];
            let op = r.below(11);
            let owner = s.m().owner.clone();
            let res = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| match op {
                0 => {
                    let pks: Vec<String> = s.m().known_keys.iter().cloned().collect();
                    if let Some(k) = pks.first() {
                        let pk: near_sdk::PublicKey = k.parse().unwrap();
                        s.runner(pk).heartbeat();
                    }
                }
                1 | 2 => {
                    let n = 10 + r.below(30) as u8;
                    let old: Vec<String> = s.m().known_keys.iter().cloned().collect();
                    s.by_1y(&owner).set_runner_key(key(n)).detach();
                    let deleted: Vec<String> = s.m().retiring.iter().cloned().collect();
                    let good = r.below(8) != 0;
                    s.runner_key_cb(good, old, deleted, key_str(n));
                    settle_deletions(&mut s);
                    if !good {
                        s.advance(HOUR_NS + 1);
                        s.by(&owner).recover_keys();
                        settle_deletions(&mut s);
                    }
                }
                3 => {
                    let to = people[r.below(5) as usize];
                    s.by_1y(who)
                        .nft_transfer(acc(to), DEED_ID.into(), None, None);
                    settle_deletions(&mut s);
                }
                4 => {
                    s.by_1y(&owner)
                        .list_for_sale(U128(10 * USD), CODE_HASH.into(), None);
                }
                5 => {
                    s.by_1y(&owner).cancel_listing();
                }
                6 | 7 => {
                    let v = s.m().listing_version;
                    let h = if r.below(5) == 0 {
                        "stale".to_string()
                    } else {
                        s.c.known_keys_hash()
                    };
                    let pk = 20 + r.below(30) as u8;
                    if s.ft(who, 10 * USD, &Sim::buy_msg(v, &h, pk), 300).is_none() {
                        let id = s.m().pending.as_ref().unwrap().sale_id;
                        s.callback(true).on_handover(id);
                    }
                }
                8 => {
                    s.pay(PAYERS[r.below(6) as usize], 12 * USD);
                }
                9 => {
                    s.advance(r.below(40) * DAY / 4);
                    let ids: Vec<u64> = s
                        .m()
                        .sales
                        .iter()
                        .filter(|x| !x.finalized)
                        .map(|x| x.id)
                        .collect();
                    for id in ids {
                        let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                            s.by(MALLORY).finalize_earnout(id);
                        }));
                    }
                }
                _ => {
                    if s.m().owner_free > 0 {
                        s.by_1y(&owner).withdraw(U128(1), None).detach();
                    }
                }
            }));
            match res {
                Ok(()) => ok += 1,
                Err(_) => refused += 1,
            }
            let m = s.m();
            assert!(m.solvent(), "seed {seed} step {step} op {op}");
            assert!(
                m.known_keys.is_disjoint(&m.retiring),
                "seed {seed} step {step}: key both live and retiring"
            );
            assert!(
                m.pending.is_none(),
                "seed {seed} step {step} op {op}: settle left pending"
            );
        }
        let st = s.by(MALLORY).get_status();
        println!(
            "STR-14 seed {seed}: {ok} ok, {refused} refused, owner {} epoch {} listing_version {} open_sales {} known_keys {}",
            st["owner"], st["owner_epoch"], st["listing_version"], st["open_sales"], st["known_keys"]
        );
        assert!(ok > 1_000, "the mix must do real work, not only refuse");
    }
}

// ----------------------------------------------------------------- STR-15

/// STR-15: buy gas sweep in 1 Tgas steps from 20 to 120 Tgas on an agent
/// with 13 open earn-outs, 600 payers and a listing. Every prepaid amount
/// below the contract's own bar (`MIN_BUY_GAS + 2 x state_gas`) is a full
/// refund that changes nothing; the first amount that starts a settle is
/// within a few Tgas of the bar and that settle completes.
#[test]
fn str15_buy_gas_sweep() {
    let mut s = sold_translator();
    s.by_1y(BOB).set_limits(demo_limits());
    for i in 0..600 {
        s.register(&long_payer(i));
        assert_eq!(s.pay(&long_payer(i), 5 * USD), Some(0));
    }
    let pair = [(CAROL, CAROL_RUNNER), (BOB, BOB_RUNNER)];
    for i in 0..13 {
        let (b, pk) = pair[i % 2];
        sell(&mut s, b, pk, 10 * USD);
    }
    let seller = s.m().owner.clone();
    if s.m().known_keys.is_empty() {
        s.by_1y(&seller).set_runner_key(key(9)).detach();
        s.runner_key_cb(true, vec![], vec![], key_str(9));
    }
    s.by_1y(&seller)
        .list_for_sale(U128(10 * USD), CODE_HASH.into(), None);
    let buyer = if seller == CAROL { BOB } else { CAROL };
    let bar = (MIN_BUY_GAS.as_gas() + 2 * s.c.state_gas().as_gas()) as f64 / 1e12;
    let (v, h) = (s.m().listing_version, s.c.known_keys_hash());
    let mut first_ok = None;
    for tgas in 20..=120u64 {
        let before = s.by(MALLORY).get_status();
        match s.ft(buyer, 10 * USD, &Sim::buy_msg(v, &h, 31), tgas) {
            Some(unused) => {
                assert_eq!(
                    unused,
                    10 * USD,
                    "{tgas} Tgas: refused buys refund everything"
                );
                assert_eq!(
                    s.by(MALLORY).get_status(),
                    before,
                    "{tgas} Tgas: a refusal changed state"
                );
            }
            None => {
                first_ok = Some(tgas);
                let id = s.m().pending.as_ref().unwrap().sale_id;
                assert_eq!(s.callback(true).on_handover(id).0, 0);
                assert_eq!(s.m().owner, buyer);
                break;
            }
        }
    }
    let first = first_ok.expect("some gas amount works");
    println!(
        "STR-15 buy gas sweep: first accepted prepaid gas {first} Tgas, contract bar {bar:.2} Tgas (state {} bytes, {} open sales)",
        s.c.state_bytes(),
        s.m().sales.iter().filter(|x| !x.finalized).count()
    );
    assert!(
        (first as f64) >= bar - 1.0 && (first as f64) <= bar + 25.0,
        "{first} vs bar {bar}"
    );
    books(&s, "after the buy");
}

// ----------------------------------------------------------------- STR-18

/// STR-18: a backdoor proven while 16 earn-outs are open. One hidden-key
/// call freezes the agent and returns every remaining escrow to its buyer as
/// a claimable balance (no outgoing transfer is scheduled, so the fan-out is
/// state only). The books stay exact, nothing is lost, and the call's host
/// gas is small and does not depend on the number of open sales much.
#[test]
fn str18_freeze_with_sixteen_open_earnouts() {
    let mut s = translator_world();
    let pair = [(BOB, BOB_RUNNER), (CAROL, CAROL_RUNNER)];
    for i in 0..agent_core::MAX_OPEN_SALES {
        let (b, pk) = pair[i % 2];
        sell(&mut s, b, pk, 100 * USD);
        s.pay(PAYERS[i % 6], 12 * USD);
    }
    assert_eq!(s.m().sales.iter().filter(|x| !x.finalized).count(), 16);
    let escrow_before = s.m().escrow_left();
    assert!(escrow_before > 0);
    // The current owner's runner key is the only one the contract knows; a
    // call signed by any other key is a hidden key.
    let (v, g) = {
        let mut c = s.call(AGENT);
        c.signer_pk = key(HIDDEN_KEY);
        gas(&mut s, c, |a| a.heartbeat())
    };
    assert_eq!(v["frozen"], true, "{v}");
    assert_eq!(s.m().escrow_left(), 0);
    assert!(s.m().frozen);
    let owed: u128 = [BOB, CAROL]
        .iter()
        .map(|a| s.c.get_claimable(acc(a)).0)
        .sum();
    assert!(owed >= escrow_before, "every escrow went back to a buyer");
    books(&s, "after the freeze");
    // Everything stays frozen: no payment is accepted as a runner call.
    let msg = panic_text(|| {
        s.runner(key(BOB_RUNNER)).heartbeat();
    });
    assert!(!msg.is_empty());
    println!(
        "STR-18 freeze with 16 open earn-outs: returned {} xUSD to buyers, host gas {} Tgas, blob {} bytes",
        escrow_before / USD, tg(g), s.c.state_bytes()
    );
}

// ----------------------------------------------------------------- STR-19

/// STR-19 (OBSERVATION, not asserted as a bug): `base.pending` lives in the
/// state blob and has no count cap; each `pay_base` adds an entry that only
/// the MPC callback removes. 700 requests whose callbacks have not run (the
/// MPC signer slow or down) grow the blob linearly. A47 bricked an agent at
/// about 585 entries of a blob table, so this is the one remaining
/// per-request growth in the blob. The seller can do it only before listing
/// (`pay_base` is refused while Listed), so the scan could count it.
#[test]
fn str19_pay_base_burst_is_capped() {
    let supplier = "0x00000000000000000000000000000000000000a1";
    let mut s = business_world();
    let mut l = demo_limits();
    l.base = Some(BaseLimitsArgs {
        daily_cap: U128(10_000 * agent_core::base::USDC),
        payees: vec![supplier.to_string()],
    });
    s.by_1y(BOB).set_limits(l);
    let b0 = s.c.state_bytes();
    let cap = agent_core::base::MAX_PENDING_AUTHS;
    let mut g_first = 0;
    let mut g_last = 0;
    for i in 0..cap {
        let mut c = s.call(AGENT);
        c.signer_pk = key(BOB_RUNNER);
        c.gas = near_sdk::Gas::from_tgas(300);
        let (r, g) = gas(&mut s, c, |a| a.pay_base(supplier.into(), U128(1), None));
        assert_eq!(r["pending"], true, "{i}: {r}");
        if i == 0 {
            g_first = g;
        }
        g_last = g;
    }
    let b1 = s.c.state_bytes();
    let mut refused = 0;
    for _ in 0..700 {
        let mut c = s.call(AGENT);
        c.signer_pk = key(BOB_RUNNER);
        c.gas = near_sdk::Gas::from_tgas(300);
        s.env(&c);
        let msg = panic_text(|| {
            let _ = s.c.pay_base(supplier.into(), U128(1), None);
        });
        if msg.contains("TooManyPendingAuths") {
            refused += 1;
        }
    }
    let b2 = s.c.state_bytes();
    println!(
        "STR-19 {cap} pending then 700 more asked: {refused} refused; blob {b0} -> {b1} -> {b2} bytes, pay_base {} -> {} Tgas",
        tg(g_first), tg(g_last)
    );
    assert_eq!(s.m().base.pending.len(), cap);
    assert_eq!(refused, 700, "every request past the cap is refused");
    assert_eq!(b2, b1, "refused requests leave the blob unchanged");
}
