//! Shares conservation (docs/SHARES.md, "The one rule that covers money"):
//! over random sequences of offer, buy, customer and self payments, claims,
//! owner withdrawals, time and closing, every token the agent holds stays in
//! exactly one bucket, the agent stays solvent, open slices never exceed
//! MAX_SHARES_BPS, and an investor is never owed more than was paid in.

use agent_core::*;
use proptest::prelude::*;

const T0: Ts = 1_767_225_600 * NANOS_PER_SEC;
const PK: &str = "ed25519:alice-runner";
const INVESTORS: [&str; 3] = ["dave", "erin", "frank"];

#[derive(Debug, Clone)]
enum Op {
    Offer { bps: u16, price: u16, days: u16 },
    Buy { who: u8, part: u16 },
    Pay { usd: u16, by_owner: bool },
    Claim { who: u8 },
    Withdraw { pct: u8 },
    Wait { days: u16 },
    Close { pick: u8 },
}

fn op() -> impl Strategy<Value = Op> {
    prop_oneof![
        2 => (1u16..6_000, 1u16..2_000, 1u16..400).prop_map(|(bps, price, days)| Op::Offer { bps, price, days }),
        3 => (0u8..3, 1u16..6_000).prop_map(|(who, part)| Op::Buy { who, part }),
        5 => (1u16..500, prop::bool::weighted(0.3)).prop_map(|(usd, by_owner)| Op::Pay { usd, by_owner }),
        2 => (0u8..3).prop_map(|who| Op::Claim { who }),
        1 => (1u8..=100).prop_map(|pct| Op::Withdraw { pct }),
        2 => (0u16..120).prop_map(|days| Op::Wait { days }),
        1 => any::<u8>().prop_map(|pick| Op::Close { pick }),
    ]
}

fn cases() -> u32 {
    std::env::var("PROP_CASES")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(256)
}

fn check(m: &Machine, paid_in: Amount, claimed: Amount) {
    assert_eq!(
        m.held,
        m.escrow_left()
            + m.upfront_held_total()
            + m.claimable_total()
            + m.owner_free
            + m.treasury
            + m.share_escrow,
        "conservation"
    );
    assert!(m.solvent(), "solvent");
    assert!(
        m.shares_bps() <= shares::MAX_SHARES_BPS,
        "slices over the cap"
    );
    let owed: Amount = INVESTORS.iter().map(|w| m.claimable_of(w)).sum();
    assert!(
        owed + claimed <= paid_in,
        "investors owed more than was paid in"
    );
}

proptest! {
    #![proptest_config(ProptestConfig { cases: cases(), ..ProptestConfig::default() })]
    #[test]
    fn shares_conserve_tokens(ops in prop::collection::vec(op(), 1..60)) {
        let mut m = Machine::new_born("translator", "alice");
        let deleted = m.set_runner_key("alice", PK, T0).unwrap();
        m.runner_key_done(m.key_batch, &[], &deleted, PK, true);
        let mut now = T0;
        let mut ids: Vec<u64> = vec![];
        // Everything that ever entered: share prices and payments.
        let mut paid_in: Amount = 0;
        let mut claimed: Amount = 0;
        for o in ops {
            match o {
                Op::Offer { bps, price, days } => {
                    if let Ok(id) = m.offer_share("alice", bps, price as Amount * USD, now + days as Ts * DAY_NS, now) {
                        ids.push(id);
                    }
                }
                Op::Buy { who, part } => {
                    if let Some(&id) = ids.last() {
                        if let Some(off) = m.offer(id) {
                            let t = off.terms();
                            let amount = off.cost(part);
                            if let Ok(spent) = m.buy_share(INVESTORS[who as usize], amount, t, part, now) {
                                paid_in += spent;
                            }
                        }
                    }
                }
                Op::Pay { usd, by_owner } => {
                    let payer = if by_owner { "alice" } else { "customer" };
                    let amount = usd as Amount * USD;
                    if !matches!(m.pay(payer, amount, false, now), PayResult::Refund(_)) {
                        paid_in += amount;
                    }
                }
                Op::Claim { who } => {
                    if let Ok(a) = m.claim(INVESTORS[who as usize]) { claimed += a; }
                }
                Op::Withdraw { pct } => {
                    let a = m.owner_free * pct as Amount / 100;
                    if a > 0 { let _ = m.withdraw("alice", a, "alice"); }
                }
                Op::Wait { days } => now += days as Ts * DAY_NS,
                Op::Close { pick } => {
                    if !ids.is_empty() {
                        let id = ids[pick as usize % ids.len()];
                        if let Ok(refunds) = m.close_share(id, now) {
                            // Refunds become claimable to investors; count them
                            // as paid in already (they came from share prices).
                            let _ = refunds;
                        }
                    }
                }
            }
            check(&m, paid_in, claimed);
        }
    }
}
