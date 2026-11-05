//! `make fuzz`: property-based fuzzing (proptest, fixed seed per run) of the
//! inputs outsiders control. Each target must never panic where the contract
//! promises an answer, and must leave the books solvent.
//!
//! - fuzz_buy_msg: `ft_on_transfer` msg = random bytes, random JSON, and
//!   mutations of real pay/buy msgs (E10). Answer: a full refund, an accepted
//!   payment, or a buy in flight; never a panic.
//! - fuzz_eip191: NEAR AI proofs with random fields and signatures (E13):
//!   never verified unless it is the real fixture, never a panic.
//! - fuzz_limits_args: `set_limits` JSON with random fields and amounts (E33):
//!   parse error or `BadLimits` or accepted; never a broken split.
//! - fuzz_mpc_answer: random MPC `sign` answers to `on_base_signed` (E22):
//!   `sign_failed`, never a panic, the cap is given back.
//!
//! Cases: FUZZ_CASES (default 64 per target in `make test`; `make fuzz` runs 3,000).

use crate::sim::*;
use crate::*;
use proptest::prelude::*;

fn cases() -> u32 {
    std::env::var("FUZZ_CASES")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(64)
}

fn no_panic<T>(f: impl FnOnce() -> T) -> Result<T, String> {
    std::panic::catch_unwind(std::panic::AssertUnwindSafe(f)).map_err(|e| {
        e.downcast_ref::<String>()
            .cloned()
            .or_else(|| e.downcast_ref::<&str>().map(|s| s.to_string()))
            .unwrap_or_default()
    })
}

/// JSON values that look like msgs.
fn json_value() -> impl Strategy<Value = Value> {
    let leaf = prop_oneof![
        Just(Value::Null),
        any::<bool>().prop_map(Value::from),
        any::<i64>().prop_map(Value::from),
        any::<u64>().prop_map(Value::from),
        "[a-z_:0-9 .]{0,40}".prop_map(Value::from),
        prop_oneof![Just("pay"), Just("buy"), Just("Pay"), Just("")].prop_map(Value::from),
    ];
    leaf.prop_recursive(3, 24, 6, |inner| {
        prop_oneof![
            prop::collection::vec(inner.clone(), 0..4).prop_map(Value::from),
            prop::collection::btree_map(
                prop_oneof![
                    Just("kind".to_string()),
                    Just("job_id".to_string()),
                    Just("nearai".to_string()),
                    Just("listing_version".to_string()),
                    Just("known_keys_hash".to_string()),
                    Just("buyer_runner_pk".to_string()),
                    "[a-z_]{1,8}",
                ],
                inner,
                0..6
            )
            .prop_map(|m| Value::Object(m.into_iter().collect())),
        ]
    })
}

fn msg() -> impl Strategy<Value = String> {
    prop_oneof![
        prop::collection::vec(any::<u8>(), 0..200)
            .prop_map(|b| String::from_utf8_lossy(&b).to_string()),
        json_value().prop_map(|v| v.to_string()),
        (any::<u64>(), "[a-f0-9]{0,70}", any::<u8>())
            .prop_map(|(v, h, pk)| Sim::buy_msg(v, &h, pk)),
        (0usize..10, 0usize..400, any::<u8>()).prop_map(|(i, at, byte)| {
            let mut m = job_msg(&job(i)).into_bytes();
            if !m.is_empty() {
                let at = at % m.len();
                m[at] = byte;
            }
            String::from_utf8_lossy(&m).to_string()
        }),
        Just("x".repeat(MAX_MSG_LEN + 1)),
    ]
}

proptest! {
    #![proptest_config(ProptestConfig { cases: cases(), failure_persistence: None, ..ProptestConfig::default() })]

    #[test]
    fn fuzz_buy_msg(m in msg(), amount in 0u128..2_000_000_000, listed in any::<bool>()) {
        let mut s = if listed { listed_translator() } else { translator_world() };
        let before = s.m().held;
        let r = no_panic(|| s.ft(PAYERS[0], amount, &m, 300));
        prop_assert!(r.is_ok(), "panicked on msg {:?}: {:?}", m, r);
        match r.unwrap() {
            Some(unused) => {
                prop_assert!(unused == 0 || unused == amount, "partial refund {unused} of {amount}");
                if unused == amount { prop_assert_eq!(s.m().held, before); }
            }
            None => prop_assert!(listed, "a buy started on an unlisted agent"),
        }
        prop_assert!(s.m().solvent());
    }

    #[test]
    fn fuzz_eip191(model in "[ -~]{0,30}", resp in "[0-9a-f]{0,70}", sig in "[0-9a-fA-Fx]{0,140}", i in 0usize..10, keep_req in any::<bool>()) {
        let mut s = translator_world();
        let j = job(i);
        let request = if keep_req { j["request"].clone() } else { Value::from(model.clone()) };
        let m = json!({"kind": "pay", "job_id": j["job_id"], "nearai": {
            "model": model, "request": request, "response_sha256": resp, "signature": sig}});
        let r = no_panic(|| s.ft(PAYERS[0], 12 * USD, &m.to_string(), 100));
        prop_assert!(r.is_ok(), "{:?}", r);
        let ev = events("payment");
        if let Some(p) = ev.first() {
            prop_assert_eq!(&p["verified"], &json!(false), "a forged proof verified");
        }
    }

    #[test]
    fn fuzz_limits_args(v in json_value(), floor in any::<u128>(), cap in any::<u128>(), o in any::<u16>(), t in any::<u16>(), payees in prop::collection::vec("[a-z.]{0,12}", 0..20)) {
        let mut s = translator_world();
        let random = serde_json::from_value::<LimitsArgs>(v);
        if let Ok(l) = random {
            let _ = no_panic(|| s.by_1y(ALICE).set_limits(l));
        }
        let j = json!({"price_floor": floor.to_string(), "daily_cap": cap.to_string(),
                       "allowed_payees": payees, "split": {"owner_bps": o, "treasury_bps": t}});
        if let Ok(l) = serde_json::from_value::<LimitsArgs>(j) {
            let r = no_panic(|| s.by_1y(ALICE).set_limits(l));
            if r.is_ok() {
                let lim = &s.m().limits;
                prop_assert_eq!(lim.owner_bps as u32 + lim.treasury_bps as u32, 10_000);
                prop_assert!(lim.daily_cap <= agent_core::policy::MAX_LIMIT);
            } else {
                let e = r.unwrap_err();
                prop_assert!(e.contains("BadLimits"), "{}", e);
            }
        }
    }

    #[test]
    fn fuzz_mpc_answer(raw in prop::collection::vec(any::<u8>(), 0..300), v in json_value(), use_json in any::<bool>()) {
        let mut s = translator_world();
        let mut l = demo_limits();
        l.base = Some(BaseLimitsArgs { daily_cap: U128(5_000_000), payees: vec!["0x00000000000000000000000000000000000000a1".into()] });
        s.by_1y(ALICE).set_limits(l);
        let r = s.runner(key(ALICE_RUNNER)).pay_base("0x00000000000000000000000000000000000000a1".into(), U128(1_000_000), None);
        let id = r["auth_id"].as_u64().unwrap();
        let mut c = s.call(AGENT);
        c.result = Some(true);
        c.result_data = if use_json { v.to_string().into_bytes() } else { raw };
        s.env(&c);
        let out = no_panic(|| s.c.on_base_signed(id));
        prop_assert!(out.is_ok(), "{:?}", out);
        let out = out.unwrap();
        prop_assert_eq!(&out["ok"], &json!(false));
        // C29: an unreadable answer keeps the cap; a readable wrong one frees it.
        let spent = s.by(BOB).get_base_wallet()["spent_today"].clone();
        if out.get("sign_unverified").is_some() {
            prop_assert_eq!(spent, json!("1000000"));
        } else {
            prop_assert_eq!(spent, json!("0"));
        }
    }
}
