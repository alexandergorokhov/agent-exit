//! I8 (feature 5): over random sequences of Base payments, sign results,
//! days, limit changes and deed transfers:
//! - authorized today (per owner) never exceeds the Base daily cap;
//! - every signed authorization is covered by `open_auth_until`;
//! - every authorization pays an allowed payee from the wallet address,
//!   valid at most an hour, and nonces never repeat.

use agent_core::base::{BaseLimits, MAX_VALID_SECS, USDC};
use agent_core::{Machine, DAY_NS, NANOS_PER_SEC};
use proptest::prelude::*;
use std::collections::BTreeSet;

const PAYEE: &str = "0x00000000000000000000000000000000000000a1";
const ADDR: &str = "0x00000000000000000000000000000000000000f1";

#[derive(Debug, Clone)]
enum Op {
    Pay { usdc: u8, secs: u16, payee_ok: bool },
    Finish { pick: u8, ok: bool },
    Wait { secs: u32 },
    Cap { usdc: u8 },
    NewOwner,
}

fn op() -> impl Strategy<Value = Op> {
    prop_oneof![
        4 => (0u8..4, 0u16..4_000, prop::bool::weighted(0.9)).prop_map(|(usdc, secs, payee_ok)| Op::Pay { usdc, secs, payee_ok }),
        4 => (any::<u8>(), prop::bool::weighted(0.7)).prop_map(|(pick, ok)| Op::Finish { pick, ok }),
        2 => (0u32..200_000).prop_map(|secs| Op::Wait { secs }),
        1 => (0u8..10).prop_map(|usdc| Op::Cap { usdc }),
        1 => Just(Op::NewOwner),
    ]
}

fn cases() -> u32 {
    std::env::var("PROP_CASES")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(256)
}

proptest! {
    #![proptest_config(ProptestConfig { cases: cases(), ..ProptestConfig::default() })]
    #[test]
    fn i8_base_wallet_stays_inside_its_limits(ops in prop::collection::vec(op(), 1..60)) {
        let mut m = Machine::new_born("translator.testnet", "alice.testnet");
        m.base.address = Some(ADDR.into());
        let mut owner = "alice.testnet".to_string();
        let set = |m: &mut Machine, owner: &str, cap: u128| {
            m.set_base_limits(owner, BaseLimits { daily_cap: cap, payees: [PAYEE.to_string()].into() }).unwrap();
        };
        set(&mut m, &owner, 5 * USDC);
        let mut now = 1_767_225_600 * NANOS_PER_SEC;
        let mut nonces = BTreeSet::new();
        let mut signed_until = 0u64;
        for o in ops {
            match o {
                Op::Pay { usdc, secs, payee_ok } => {
                    let to = if payee_ok { PAYEE } else { "0x00000000000000000000000000000000000000b2" };
                    let before = m.clone();
                    match m.begin_base_pay(to, usdc as u128 * USDC, secs as u64, now) {
                        Ok(p) => {
                            prop_assert_eq!(&p.auth.from, ADDR);
                            prop_assert_eq!(&p.auth.to, PAYEE);
                            prop_assert!(secs as u64 >= 1 && secs as u64 <= MAX_VALID_SECS);
                            prop_assert!(nonces.insert(p.auth.nonce.clone()), "nonce reused");
                            prop_assert!(m.base.spent_today(m.owner_epoch, now) <= m.base.limits.daily_cap, "over the Base cap");
                        }
                        Err(_) => prop_assert_eq!(&m, &before, "a refusal changed state"),
                    }
                }
                Op::Finish { pick, ok } => {
                    let ids: Vec<u64> = m.base.pending.keys().copied().collect();
                    if !ids.is_empty() {
                        let id = ids[pick as usize % ids.len()];
                        let a = m.finish_base_pay(id, ok).unwrap();
                        if ok {
                            signed_until = signed_until.max(a.valid_before * NANOS_PER_SEC);
                        }
                    }
                }
                Op::Wait { secs } => now += secs as u64 * NANOS_PER_SEC,
                Op::Cap { usdc } => set(&mut m, &owner, usdc as u128 * USDC),
                Op::NewOwner => {
                    // A deed move outside buy() needs no open auths; model the
                    // owner change directly (limits move with the deed).
                    m.owner_epoch += 1;
                    owner = format!("o{}.testnet", m.owner_epoch);
                    m.owner = owner.clone();
                }
            }
            let spent = m.base.spent_today(m.owner_epoch, now);
            let reserved: u128 = m.base.pending.values()
                .chain(m.base.signed.iter())
                .filter(|a| a.owner_epoch == m.owner_epoch && a.day == now / DAY_NS)
                .map(|a| a.value).sum();
            prop_assert!(spent <= m.base.limits.daily_cap.max(reserved));
            prop_assert!(m.open_auth_until >= signed_until, "a signed authorization is not covered");
            for a in m.base.pending.values() {
                prop_assert!(m.open_auth_until >= a.valid_before * NANOS_PER_SEC);
            }
        }
    }
}
