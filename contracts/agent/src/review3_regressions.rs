//! Security review 3: the reviewer's scratch tests, with the asserts
//! inverted (they asserted the defect). Scenario ids A43 and up.

use crate::sim::*;
use crate::*;

/// Built agent wasm size the reviewer measured (code is part of the
/// account's storage on chain).
const WASM_BYTES: u64 = 439_472;

/// A43 (M1, R3-1 inverted): a runner `pay` at the documented
/// `RUNNER_GAS_PAY` works on an account whose storage includes the code.
#[test]
fn a43_pay_at_documented_gas_with_code_in_storage() {
    let mut s = business_world();
    let blob = near_sdk::borsh::to_vec(&s.c).unwrap().len() as u64;
    assert_eq!(blob, s.c.state_bytes());
    let mut c = s.call(AGENT);
    c.signer_pk = key(BOB_RUNNER);
    c.gas = RUNNER_GAS_PAY;
    c.storage = WASM_BYTES + blob + 1_000 * 73;
    s.env(&c);
    let v = s.c.pay(acc(SUMMARIZER), U128(USD));
    assert_eq!(v["ok"], true);
}

/// A43 (M1): with the largest state the bounded tables allow (1,000 payers,
/// a full paid_to quota) a pay still fits `RUNNER_GAS_PAY`.
#[test]
fn a43_pay_fits_with_full_tables() {
    let mut s = business_world();
    for i in 0..1000 {
        s.pay(
            &format!("payer-with-a-longish-name-{i:04}.testnet"),
            5 * USD,
        );
    }
    for i in 0..agent_core::MAX_PAID_TO - 1 {
        s.by_1y(BOB)
            .withdraw(
                U128(1),
                Some(acc(&format!("destination-number-{i:04}.testnet"))),
            )
            .detach();
    }
    let mut c = s.call(AGENT);
    c.signer_pk = key(BOB_RUNNER);
    c.gas = RUNNER_GAS_PAY;
    c.storage = WASM_BYTES + s.c.state_bytes();
    s.env(&c);
    let v = s.c.pay(acc(SUMMARIZER), U128(USD));
    assert_eq!(v["ok"], true);
}

/// A43 (M1, R3-2 inverted): a large account (code plus many used_sigs
/// entries) does not make a buy impossible: the gas bar counts the state
/// blob, not the account's storage.
#[test]
fn a43_large_account_storage_still_sells() {
    let mut s = listed_translator();
    let v = s.m().listing_version;
    let h = s.c.known_keys_hash();
    let mut c = s.call(TOKEN);
    c.signer = BOB.into();
    c.gas = near_sdk::Gas::from_tgas(300);
    c.storage = 2_200_000;
    s.env(&c);
    match s
        .c
        .ft_on_transfer(acc(BOB), U128(1_000 * USD), Sim::buy_msg(v, &h, BOB_RUNNER))
    {
        PromiseOrValue::Promise(_) => {}
        PromiseOrValue::Value(u) => panic!("refused, unused {}: {:?}", u.0, events("refused")),
    }
}
