//! Meter details: fresh-payer reporting (informational) and E16 pooling.

use agent_core::*;

const T0: Ts = 1_767_225_600 * NANOS_PER_SEC;

#[test]
fn fresh_payers_are_reported_but_still_counted() {
    let mut m = Machine::new("translator", "alice");
    m.pay("old.testnet", 10 * USD, false, T0);
    m.pay("new.testnet", 10 * USD, false, T0 + 9 * DAY_NS);
    let e = m.earnings(T0 + 10 * DAY_NS);
    // `new` first paid 1 day ago (< 7 days): fresh. `old` 10 days ago: not.
    assert_eq!(e.fresh_payers, 1);
    assert_eq!(e.counted_from_fresh_payers, 10 * USD);
    // Fresh is a warning for the buyer, not a flag: both payments count (N3).
    assert_eq!(e.counted, 20 * USD);
    let later = m.earnings(T0 + 20 * DAY_NS);
    assert_eq!(later.fresh_payers, 0);
}

#[test]
fn e16_payer_cap_pools_into_other() {
    let mut m = Machine::new("translator", "alice");
    for i in 0..meter::MAX_PAYERS {
        m.pay(&format!("payer{i}.testnet"), USD, false, T0);
    }
    let before = m.meter.payers.len();
    m.pay("payer-1001.testnet", USD, false, T0);
    assert_eq!(m.meter.payers.len(), before);
    assert_eq!(m.meter.other.totals.gross, USD);
    // Known payers still get their own row.
    m.pay("payer0.testnet", USD, false, T0);
    assert_eq!(m.meter.payers["payer0.testnet"].totals.payments, 2);
}
