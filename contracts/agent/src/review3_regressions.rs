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

fn verify_note() -> String {
    events("payment")[0]["verify"].as_str().unwrap().to_string()
}

const HOUR: u64 = 3_600 * agent_core::NANOS_PER_SEC;

// ------------------------------------------------------------ L1 (A44)

/// A44 (L1, R3-3 inverted): a rotation callback that lands after
/// `recover_keys` is stale. It does not put the deleted key back into
/// `known_keys`; the agent stays paused until the owner sets a key, and the
/// next rotation works.
#[test]
fn a44_late_rotation_callback_after_recover_keys_is_stale() {
    let mut s = sold_translator();
    s.by_1y(BOB).set_runner_key(key(CAROL_RUNNER)).detach();
    let batch = s.m().key_batch;
    s.advance(HOUR + 1);
    let r = s.by_1y(BOB).recover_keys();
    let deleted: Vec<String> = serde_json::from_value(r["delete_keys"].clone()).unwrap();
    assert!(deleted.contains(&key_str(CAROL_RUNNER)));
    assert_ne!(s.m().key_batch, batch, "recovery bumps the nonce");
    // The original batch had succeeded; its callback arrives late.
    let applied = s.callback(true).on_runner_key(
        vec![key_str(BOB_RUNNER)],
        vec![key_str(BOB_RUNNER)],
        key_str(CAROL_RUNNER),
        batch,
    );
    assert!(!applied);
    assert_eq!(events("stale_callback")[0]["callback"], "on_runner_key");
    s.callback(true).on_keys_deleted(deleted);
    assert!(
        !s.m().known_keys.contains(&key_str(CAROL_RUNNER)),
        "no phantom key"
    );
    assert!(s.m().known_keys.is_empty() && s.m().incoming.is_empty() && s.m().retiring.is_empty());
    assert!(s.m().paused);
    // The owner rotates again and it works.
    s.by_1y(BOB).set_runner_key(key(9)).detach();
    s.runner_key_cb(true, vec![], vec![], key_str(9));
    assert_eq!(s.runner(key(9)).heartbeat()["ok"], true);
}

/// A44 (L1): the batch itself ran after the recovery's deletions, so it
/// added a key no set names. The stale callback marks it `retiring` (a
/// signature by it is `NotRunner`, not a backdoor) and deletes it.
#[test]
fn a44_stale_rotation_batch_key_is_deleted_not_a_backdoor() {
    let mut s = sold_translator();
    s.by_1y(BOB).set_runner_key(key(CAROL_RUNNER)).detach();
    let batch = s.m().key_batch;
    s.advance(HOUR + 1);
    let r = s.by_1y(BOB).recover_keys();
    let deleted: Vec<String> = serde_json::from_value(r["delete_keys"].clone()).unwrap();
    s.callback(true).on_keys_deleted(deleted);
    assert!(!s.m().key_tracked(&key_str(CAROL_RUNNER)));
    s.callback(true)
        .on_runner_key(vec![], vec![], key_str(CAROL_RUNNER), batch);
    assert_eq!(
        key_actions(AGENT),
        vec![("delete".to_string(), key_str(CAROL_RUNNER))]
    );
    assert!(s.m().retiring.contains(&key_str(CAROL_RUNNER)));
    assert!(s.m().known_keys.is_empty());
    // Until the deletion is confirmed, the key cannot act and does not freeze.
    assert_eq!(
        panic_text(|| {
            s.runner(key(CAROL_RUNNER)).heartbeat();
        }),
        "NotRunner"
    );
    assert!(!s.m().frozen);
    s.callback(true)
        .on_keys_deleted(vec![key_str(CAROL_RUNNER)]);
    assert!(s.m().retiring.is_empty());
}

/// A44 (L1): `on_handover` for a buy `recover_sale` undid refunds the
/// buyer in full without panicking, and deletes the buyer key its batch
/// added after the recovery. Nothing else changes.
#[test]
fn a44_late_handover_after_recover_sale_is_stale() {
    let mut s = listed_translator();
    assert_eq!(s.buy(BOB, 1_000 * USD, BOB_RUNNER), None);
    let sale_id = s.m().pending.as_ref().unwrap().sale_id;
    s.advance(HOUR + 1);
    let r = s.by(MALLORY).recover_sale();
    let deleted: Vec<String> = serde_json::from_value(r["delete_keys"].clone()).unwrap();
    s.callback(true).on_keys_deleted(deleted);
    let before = s.m().clone();
    let unused = s.callback(true).on_handover(sale_id);
    assert_eq!(unused.0, 1_000 * USD, "the token refunds the buyer in full");
    assert_eq!(events("stale_callback")[0]["callback"], "on_handover");
    assert_eq!(
        key_actions(AGENT),
        vec![("delete".to_string(), key_str(BOB_RUNNER))]
    );
    assert!(s.m().retiring.contains(&key_str(BOB_RUNNER)));
    assert_eq!(s.m().owner, ALICE);
    assert_eq!(s.m().held, before.held);
    assert_eq!(s.m().status, before.status);
    assert!(s.m().solvent());
    // A second late copy finds nothing to undo.
    assert!(s.m().recovered_buy.is_none());
}

// ------------------------------------------------------------ L7 (A45)

/// A45 (L7): payout destinations of an era a finalized sale closed are
/// pruned, so the table no longer lasts the agent's whole life. Moving the
/// deed without a sale prunes nothing.
#[test]
fn a45_paid_to_is_pruned_after_the_earnout_closes() {
    let mut s = translator_world();
    for i in 0..agent_core::MAX_PAID_TO {
        s.by_1y(ALICE)
            .withdraw(U128(1), Some(acc(&format!("junk{i}.testnet"))))
            .detach();
    }
    s.by_1y(ALICE)
        .list_for_sale(U128(1_000 * USD), CODE_HASH.into(), None);
    assert_eq!(s.buy(BOB, 1_000 * USD, BOB_RUNNER), None);
    s.callback(true).on_handover(1);
    // The open earn-out still needs alice's destinations (A31).
    assert_eq!(s.by(MALLORY).get_status()["paid_to_total"], 1_000);
    s.advance(31 * DAY);
    s.by(MALLORY).finalize_earnout(1);
    assert_eq!(s.by(MALLORY).get_status()["paid_to_total"], 0);
    // A deed moved between bob's own accounts keeps bob's entries.
    s.pay(PAYERS[0], 100 * USD);
    s.by_1y(BOB)
        .withdraw(U128(1), Some(acc("bobs-bank.testnet")))
        .detach();
    s.by_1y(BOB)
        .nft_transfer(acc(CAROL), DEED_ID.into(), None, None);
    assert_eq!(s.by(MALLORY).get_status()["paid_to_total"], 1);
}

// ------------------------------------------------------------ M2 (A46)

/// A46 (M2): a proof older than the window, ahead of the block, or without
/// `signed_at` is not verified (the payment still counts).
#[test]
fn a46_proof_time_window() {
    for (now, want) in [
        (T0 + 8 * DAY + 2 * HOUR, "SignatureExpired"),
        (T0 - HOUR, "SignedInFuture"),
    ] {
        let mut s = Sim::new(AGENT, ALICE);
        s.now = now;
        s.ft(PAYERS[0], 12 * USD, &job_msg(&job(0)), 100);
        assert_eq!(verify_note(), want);
        assert_eq!(events("payment")[0]["counted"], true);
    }
    let mut s = Sim::new(AGENT, ALICE);
    let mut e = job(0);
    let mut req: serde_json::Value = serde_json::from_str(e["request"].as_str().unwrap()).unwrap();
    req["metadata"].as_object_mut().unwrap().remove("signed_at");
    e["request"] = json!(req.to_string());
    s.ft(PAYERS[0], 12 * USD, &job_msg(&e), 100);
    assert_eq!(verify_note(), "NoSignedAt");
}

/// A46 (M2): the used-signature table is bounded. When it is full of
/// unexpired ids a new proof is not verified (`SigWindowFull`), and expired
/// ids make room; it never holds more than `MAX_SIGS`.
#[test]
fn a46_used_sigs_are_bounded() {
    let mut s = Sim::new(AGENT, ALICE);
    s.now = T0 + HOUR;
    let now_s = s.now / agent_core::NANOS_PER_SEC;
    s.env(&s.call(AGENT));
    for i in 0..verified::MAX_SIGS {
        let mut id = [0u8; 32];
        id[..4].copy_from_slice(&i.to_le_bytes());
        s.c.used_sigs.insert(id, now_s + 60, now_s);
    }
    assert_eq!(s.c.used_sigs.len(), verified::MAX_SIGS);
    s.ft(PAYERS[0], 12 * USD, &job_msg(&job(0)), 100);
    assert_eq!(verify_note(), "SigWindowFull");
    assert_eq!(events("payment")[0]["counted"], true);
    // The filler ids expire; the same proof now verifies.
    s.advance(120 * agent_core::NANOS_PER_SEC);
    s.ft(PAYERS[0], 12 * USD, &job_msg(&job(0)), 100);
    assert!(verify_note().starts_with("ok:"), "{}", verify_note());
    assert!(s.c.used_sigs.len() <= verified::MAX_SIGS);
    // And it is used.
    s.ft(PAYERS[0], 12 * USD, &job_msg(&job(0)), 100);
    assert_eq!(verify_note(), "SignatureUsed");
}

/// A46 (M2): `get_status.near_storage` says whether the NEAR balance
/// covers storage, the signature window's growth and the reserve, and the
/// owner cannot withdraw the growth reserve.
#[test]
fn a46_near_storage_solvency_is_shown_and_kept() {
    let mut s = business_world();
    let st = s.by(MALLORY).get_status();
    assert_eq!(st["near_storage"]["solvent"], true);
    assert_eq!(st["near_storage"]["max_sig_entries"], verified::MAX_SIGS);
    let growth: u128 = st["near_storage"]["growth_reserve"]
        .as_str()
        .unwrap()
        .parse()
        .unwrap();
    assert!(growth > 0);
    let max = s.by(MALLORY).near_withdrawable();
    assert_eq!(
        max.as_yoctonear(),
        s.balance.as_yoctonear() - growth - NEAR_RESERVE.as_yoctonear()
    );
    s.balance = NearToken::from_millinear(1_500);
    assert_eq!(s.by(MALLORY).get_status()["near_storage"]["solvent"], false);
}
