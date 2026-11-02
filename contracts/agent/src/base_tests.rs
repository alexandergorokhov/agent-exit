//! Feature 5, "the wallet follows the name": the agent's Base Sepolia wallet
//! (Chain Signatures) with the offline TEST MPC signer (`base_wallet::MockMpc`).
//! Scenario ids: S6 (mock MPC), E22, E23, A9, A20.

use crate::sim::*;
use agent_core::base::{BASE_PATH, USDC};
use base_wallet::{check_auth, MockMpc};
use near_sdk::json_types::U128;
use near_sdk::serde_json::{json, Value};

/// A Base Sepolia supplier the agent pays (TEST address).
const SUPPLIER: &str = "0x00000000000000000000000000000000000000a1";
const OTHER: &str = "0x00000000000000000000000000000000000000b2";

fn base_limits(owner: &str, s: &mut Sim) {
    let mut l = demo_limits();
    l.base = Some(crate::BaseLimitsArgs {
        daily_cap: U128(5 * USDC),
        payees: vec![SUPPLIER.to_uppercase().replace("0X", "0x")],
    });
    s.by_1y(owner).set_limits(l);
}

fn panics(f: impl FnOnce()) -> String {
    let r = std::panic::catch_unwind(std::panic::AssertUnwindSafe(f));
    let e = r.expect_err("expected a panic");
    let raw = e
        .downcast_ref::<String>()
        .cloned()
        .or_else(|| e.downcast_ref::<&str>().map(|s| s.to_string()))
        .unwrap_or_default();
    match raw.split_once("panic_msg: \"") {
        Some((_, rest)) => rest
            .rsplit_once("\" })")
            .map(|(m, _)| m)
            .unwrap_or(rest)
            .to_string(),
        None => raw,
    }
}

/// Runner asks for a Base payment; the mock MPC answers; returns the callback value.
fn pay_base(s: &mut Sim, runner: u8, amount: u128, mpc: &MockMpc) -> Value {
    let r = s
        .runner(key(runner))
        .pay_base(SUPPLIER.into(), U128(amount), None);
    assert_eq!(r["pending"], true, "{r}");
    let calls = mpc_sign_calls();
    assert_eq!(calls.len(), 1);
    let (args, deposit, _gas) = &calls[0];
    assert_eq!(args["request"]["path"], BASE_PATH);
    assert_eq!(args["request"]["domain_id"], 0);
    assert_eq!(deposit.as_yoctonear(), 1);
    let digest: [u8; 32] =
        crate::verified::hex_decode(args["request"]["payload_v2"]["Ecdsa"].as_str().unwrap())
            .unwrap()
            .try_into()
            .unwrap();
    let req = &events("base_sign_requested")[0];
    assert_eq!(
        req["digest"],
        format!("0x{}", crate::verified::hex_lower(&digest))
    );
    let answer = mpc.sign(&s.agent.clone(), BASE_PATH, &digest);
    let id = r["auth_id"].as_u64().unwrap();
    s.callback_data(&answer).on_base_signed(id)
}

#[test]
fn s6_wallet_follows_the_name() {
    let mpc = MockMpc::default();
    let mut s = translator_world();
    let address = mpc.derive_address(AGENT, BASE_PATH);
    assert_eq!(s.by(BOB).get_base_wallet()["address"], address);

    // Before the sale: alice's runner pays 1.00 USDC on Base.
    base_limits(ALICE, &mut s);
    let r = pay_base(&mut s, ALICE_RUNNER, USDC, &mpc);
    assert_eq!(r["ok"], true, "{r}");
    let ev = &events("base_auth_signed")[0];
    assert_eq!(check_auth(ev).unwrap(), address);
    assert_eq!(ev["owner_epoch"], 1);

    // A9: the authorization is valid for 600 s; until then no listing.
    let e = panics(|| {
        s.by_1y(ALICE)
            .list_for_sale(U128(1_000 * USD), CODE_HASH.into(), None);
    });
    assert_eq!(e, "OpenApproval");
    s.now += 601 * agent_core::NANOS_PER_SEC;
    s.by_1y(ALICE)
        .list_for_sale(U128(1_000 * USD), CODE_HASH.into(), None);
    // Listed: the runner cannot sign Base payments either.
    let e = panics(|| {
        s.runner(key(ALICE_RUNNER))
            .pay_base(SUPPLIER.into(), U128(USDC), None);
    });
    assert_eq!(e, "Listed: spending paused");

    // The sale.
    assert_eq!(s.buy(BOB, 1_000 * USD, BOB_RUNNER), None);
    let sale = s.m().pending.as_ref().unwrap().sale_id;
    s.callback(true).on_handover(sale);
    s.callback(true)
        .on_keys_deleted(vec![key_str(ALICE_RUNNER)]);

    // After: the same address, now signing for bob's runner only.
    let w = s.by(BOB).get_base_wallet();
    assert_eq!(w["address"], address);
    assert_eq!(w["owner"], BOB);
    // A20: the old seller cannot make the wallet sign.
    let e = panics(|| {
        s.by(ALICE).pay_base(SUPPLIER.into(), U128(USDC), None);
    });
    assert_eq!(e, "NotRunner");
    // Bob's runner, inside the limits that came with the deed.
    let r = pay_base(&mut s, BOB_RUNNER, USDC, &mpc);
    assert_eq!(r["ok"], true, "{r}");
    let ev = &events("base_auth_signed")[0];
    assert_eq!(check_auth(ev).unwrap(), address);
    assert_eq!(ev["owner_epoch"], 2);
    // Bob's cap is his own: alice's 1.00 that day does not count (E18-style).
    assert_eq!(s.by(BOB).get_base_wallet()["spent_today"], "1000000");
    // Old seller sets Base limits: NotOwner.
    let e = panics(|| base_limits(ALICE, &mut s));
    assert_eq!(e, "NotOwner");
}

#[test]
fn base_limits_cap_and_payees() {
    let mpc = MockMpc::default();
    let mut s = translator_world();
    // No Base limits yet: nobody can be paid.
    let e = panics(|| {
        s.runner(key(ALICE_RUNNER))
            .pay_base(SUPPLIER.into(), U128(USDC), None);
    });
    assert!(e.starts_with("PolicyViolation:BasePayee"), "{e}");
    base_limits(ALICE, &mut s);
    for _ in 0..5 {
        assert_eq!(pay_base(&mut s, ALICE_RUNNER, USDC, &mpc)["ok"], true);
    }
    let e = panics(|| {
        s.runner(key(ALICE_RUNNER))
            .pay_base(SUPPLIER.into(), U128(USDC), None);
    });
    assert_eq!(
        e,
        "PolicyViolation:BaseDailyCap: 5.00 + 1.00 > Base daily cap 5.00 USDC"
    );
    let e = panics(|| {
        s.runner(key(ALICE_RUNNER))
            .pay_base(OTHER.into(), U128(USDC), None);
    });
    assert_eq!(
        e,
        format!("PolicyViolation:BasePayee: {OTHER} is not an allowed Base payee")
    );
    // The next day the cap is back.
    s.now += DAY;
    assert_eq!(pay_base(&mut s, ALICE_RUNNER, USDC, &mpc)["ok"], true);
    // The agent's own address cannot be a payee; bad addresses are refused.
    for bad in [mpc.derive_address(AGENT, BASE_PATH), "0x1234".to_string()] {
        let mut l = demo_limits();
        l.base = Some(crate::BaseLimitsArgs {
            daily_cap: U128(USDC),
            payees: vec![bad],
        });
        let e = panics(|| {
            s.by_1y(ALICE).set_limits(l);
        });
        assert_eq!(e, "BadLimits");
    }
}

#[test]
fn e23_validity_at_most_an_hour() {
    let mut s = translator_world();
    base_limits(ALICE, &mut s);
    for secs in [0, 3_601, u64::MAX] {
        let e = panics(|| {
            s.runner(key(ALICE_RUNNER))
                .pay_base(SUPPLIER.into(), U128(USDC), Some(secs));
        });
        assert_eq!(e, "ValidityTooLong: at most 3600 s");
    }
    let r = s
        .runner(key(ALICE_RUNNER))
        .pay_base(SUPPLIER.into(), U128(USDC), Some(3_600));
    assert_eq!(r["pending"], true);
}

#[test]
fn e22_sign_failure_releases_the_cap_and_retries_with_a_new_nonce() {
    let mpc = MockMpc::default();
    let mut s = translator_world();
    base_limits(ALICE, &mut s);
    let r = s
        .runner(key(ALICE_RUNNER))
        .pay_base(SUPPLIER.into(), U128(5 * USDC), None);
    let first_nonce = events("base_sign_requested")[0]["nonce"].clone();
    // Pending: listing is already blocked (the signer may still answer).
    assert!(s.by(BOB).get_base_wallet()["open_auth_until"] != "0");
    let out = s
        .callback(false)
        .on_base_signed(r["auth_id"].as_u64().unwrap());
    assert_eq!(out["ok"], false);
    assert_eq!(events("sign_failed")[0]["reason"], "SignFailed");
    let w = s.by(BOB).get_base_wallet();
    assert_eq!(w["spent_today"], "0");
    assert_eq!(w["open_auth_until"], "0");
    assert_eq!(w["signed"].as_array().unwrap().len(), 0);
    // Retry: a new nonce, the full cap again.
    assert_eq!(pay_base(&mut s, ALICE_RUNNER, 5 * USDC, &mpc)["ok"], true);
    assert_ne!(events("base_auth_signed")[0]["nonce"], first_nonce);
    // A zero payment is refused before the signer is asked.
    let e = panics(|| {
        s.runner(key(ALICE_RUNNER))
            .pay_base(SUPPLIER.into(), U128(0), None);
    });
    assert_eq!(e, "InsufficientFunds");
}

#[test]
fn a_wrong_base_address_never_signs() {
    // A seller deploys with an address the MPC key does not control: every
    // signature fails the contract's check; nothing is issued.
    let mpc = MockMpc::new([9; 32]);
    let mut s = translator_world();
    base_limits(ALICE, &mut s);
    let r = pay_base(&mut s, ALICE_RUNNER, USDC, &mpc);
    assert_eq!(r["ok"], false);
    assert_eq!(events("sign_failed")[0]["reason"], "BaseAddressMismatch");
    assert_eq!(s.by(BOB).get_base_wallet()["spent_today"], "0");
    for junk in [
        json!({}),
        json!({"big_r": {"affine_point": "05ab"}, "s": {"scalar": "00"}, "recovery_id": 0}),
        json!("x"),
    ] {
        let r = s
            .runner(key(ALICE_RUNNER))
            .pay_base(SUPPLIER.into(), U128(USDC), None);
        let out = s
            .callback_data(&junk)
            .on_base_signed(r["auth_id"].as_u64().unwrap());
        assert_eq!(out["ok"], false);
        assert_eq!(events("sign_failed")[0]["reason"], "BadSignResponse");
    }
}

#[test]
fn high_s_answers_are_normalised_for_the_token() {
    let mpc = MockMpc::default().high_s();
    let mut s = translator_world();
    base_limits(ALICE, &mut s);
    assert_eq!(pay_base(&mut s, ALICE_RUNNER, USDC, &mpc)["ok"], true);
    check_auth(&events("base_auth_signed")[0]).unwrap();
}

#[test]
fn no_base_address_no_base_payments() {
    let mut s = translator_world();
    s.c.m.base.address = None;
    base_limits(ALICE, &mut s);
    let e = panics(|| {
        s.runner(key(ALICE_RUNNER))
            .pay_base(SUPPLIER.into(), U128(USDC), None);
    });
    assert_eq!(e, "BaseWalletNotSet");
}
