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
