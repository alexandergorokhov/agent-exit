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
    assert_eq!(s.m().paid_to_epoch("junk0.testnet"), None, "pruned");
    // A pruned destination paid again is a new entry of the current owner.
    s.pay(PAYERS[1], 100 * USD);
    s.by_1y(BOB)
        .withdraw(U128(1), Some(acc("junk0.testnet")))
        .detach();
    assert_eq!(
        s.m().paid_to_epoch("junk0.testnet"),
        Some(s.m().owner_epoch)
    );
    assert_eq!(s.m().paid_to_this_owner(), 1);
    assert!(s.m().flags_for("junk0.testnet").round_trip);
    s.callback(true).on_transfer_out(
        OutKind::OwnerWithdraw,
        acc("junk0.testnet"),
        U128(1),
        Some(acc(BOB)),
    );
    // A deed moved between bob's own accounts keeps bob's entries.
    s.pay(PAYERS[0], 100 * USD);
    s.by_1y(BOB)
        .withdraw(U128(1), Some(acc("bobs-bank.testnet")))
        .detach();
    s.by_1y(BOB)
        .nft_transfer(acc(CAROL), DEED_ID.into(), None, None);
    assert_eq!(s.by(MALLORY).get_status()["paid_to_total"], 2);
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
    let staked: u128 = st["near_storage"]["staked"]
        .as_str()
        .unwrap()
        .parse()
        .unwrap();
    assert!(staked > 0, "payer and day rows are real storage");
    let max = s.by(MALLORY).near_withdrawable();
    assert_eq!(
        max.as_yoctonear(),
        s.balance.as_yoctonear()
            - staked
            - growth
            - NEAR_RESERVE.as_yoctonear()
            - s.c.payer_deposits,
        "payer deposits stay too (A50)"
    );
    s.balance = NearToken::from_millinear(1_500);
    assert_eq!(s.by(MALLORY).get_status()["near_storage"]["solvent"], false);
}

// ------------------------------------------- A47: outsiders and the state blob

/// A 64-character payer id (the longest NEAR account id).
fn long_payer(i: usize) -> String {
    let id = format!("{i:0>56}.testnet");
    assert_eq!(id.len(), 64);
    id
}

/// A47 (local-node finding at 12d1ab7: about 584 distinct 64-character
/// payers made every call trap while loading state). Payer and day rows
/// live under their own storage keys; the state blob holds aggregates only,
/// so its size, and the cost of loading it, does not grow with payers.
#[test]
fn a47_state_blob_is_flat_in_the_number_of_payers() {
    let mut s = sold_translator();
    s.by_1y(BOB).set_limits(demo_limits());
    let before = s.c.state_bytes();
    let rows_before = s.m().meter.tracked_payers;
    let pooled_before = s.m().meter.other.totals.payments;
    for i in 0..1_200 {
        s.register(&long_payer(i));
        assert_eq!(
            s.pay(&long_payer(i), 5 * USD),
            Some(0),
            "{i}: {:?}",
            events("refused")
        );
    }
    let m = s.m();
    // No cap on rows (A50): every registered payer has its own row.
    assert_eq!(m.meter.tracked_payers, rows_before + 1_200);
    assert_eq!(m.meter.other.totals.payments, pooled_before);
    let after = s.c.state_bytes();
    // The aggregates are fixed width: same size (give or take an Option).
    assert!(after <= before + 16, "state blob {before} -> {after} bytes");
    assert!(after < 8_000, "{after}");
    // Every view and call still works and reads one blob plus a few rows.
    let st = s.by(MALLORY).get_status();
    assert_eq!(st["near_storage"]["payer_entries"], rows_before + 1_200);
    let e = s.c.get_earnings();
    assert_eq!(e.tracked_payers, rows_before + 1_200);
    assert!(e.days.len() as u64 <= agent_core::meter::EARNINGS_DAYS);
    let row = s.c.get_payer(acc(&long_payer(7))).expect("own row");
    assert_eq!(row.totals.gross, 5 * USD);
    let row =
        s.c.get_payer(acc(&long_payer(1_100)))
            .expect("own row, no cap (A50)");
    assert_eq!(row.totals.gross, 5 * USD);
    assert_eq!(s.runner(key(BOB_RUNNER)).heartbeat()["ok"], true);
    let v = s.runner(key(BOB_RUNNER)).pay(acc(SUMMARIZER), U128(USD));
    assert_eq!(v["ok"], true);
    // And the agent can still be sold.
    s.by_1y(BOB)
        .list_for_sale(U128(1_000 * USD), CODE_HASH.into(), None);
    assert_eq!(s.buy(CAROL, 1_000 * USD, CAROL_RUNNER), None);
    let sale_id = s.m().pending.as_ref().unwrap().sale_id;
    assert_eq!(s.callback(true).on_handover(sale_id).0, 0);
    assert_eq!(s.m().owner, CAROL);
}

/// A47: the host cost of a view (storage reads included, as the mocked
/// runtime counts it) is the same with 10 payers and with 1,000.
#[test]
fn a47_view_cost_is_flat() {
    let cost = |n: usize| {
        let mut s = translator_world();
        for i in 0..n {
            if s.m().meter.tracked_payers < 1_000 {
                s.register(&long_payer(i));
            }
            s.pay(&long_payer(i), USD);
        }
        s.env(&s.call(MALLORY));
        let g0 = near_sdk::env::used_gas();
        let _ = s.c.get_status();
        let _ = s.c.get_earnings();
        near_sdk::env::used_gas().as_gas() - g0.as_gas()
    };
    let (small, big) = (cost(10), cost(1_000));
    assert_eq!(small, big, "view gas {small} vs {big}");
}

/// A49: payments alone never add a payer row (test tokens are free); a
/// row costs `PAYER_ROW_DEPOSIT` NEAR through `storage_deposit`. Too small a
/// deposit is refused (the deposit goes back with the failed call); a
/// second registration refunds the whole deposit. (There is no table cap
/// any more, A50.)
#[test]
fn a49_payer_rows_cost_near() {
    let mut s = translator_world();
    let rows = s.m().meter.tracked_payers;
    for i in 0..50 {
        assert_eq!(s.pay(&format!("faucet-{i}.testnet"), 100 * USD), Some(0));
    }
    assert_eq!(s.m().meter.tracked_payers, rows, "no rows from payments");
    assert!(s.c.get_payer(acc("faucet-0.testnet")).is_none());
    assert!(s.c.storage_balance_of(acc("faucet-0.testnet")).is_none());
    let mut c = s.call("faucet-0.testnet");
    c.deposit = PAYER_ROW_DEPOSIT.saturating_sub(NearToken::from_yoctonear(1));
    s.env(&c);
    assert_eq!(
        panic_text(|| {
            s.c.storage_deposit(None, None);
        }),
        "StorageDepositTooSmall"
    );
    s.register("faucet-0.testnet");
    assert!(s.c.storage_balance_of(acc("faucet-0.testnet")).is_some());
    assert_eq!(s.m().meter.tracked_payers, rows + 1);
    // Again: the whole deposit comes back.
    let mut c = s.call("faucet-0.testnet");
    c.deposit = PAYER_ROW_DEPOSIT;
    s.env(&c);
    s.c.storage_deposit(None, None);
    assert_eq!(s.m().meter.tracked_payers, rows + 1);
    let refunds = near_sdk::test_utils::get_created_receipts()
        .into_iter()
        .filter(|r| r.receiver_id.as_str() == "faucet-0.testnet")
        .count();
    assert_eq!(refunds, 1, "the second deposit is refunded");
    // The deposit covers the longest row: a 64-character payer, registered
    // and then paying, grows storage by at most `PAYER_ENTRY_BYTES`.
    let long = format!("{}.testnet", "l".repeat(56));
    assert_eq!(long.len(), 64);
    s.pay(&long, 100 * USD);
    let before = near_sdk::env::storage_usage();
    s.register(&long);
    s.pay(&long, 100 * USD);
    let grew = near_sdk::env::storage_usage() as u128 - before as u128;
    assert!(
        grew > 0 && grew <= PAYER_ENTRY_BYTES + PAYER_RECORD_BYTES,
        "row and deposit record grew {grew} bytes"
    );
    let _ = rows;
}

/// A47: day rows are capped at `MAX_DAYS`; the oldest are dropped and the
/// totals stay.
#[test]
fn a47_day_rows_are_bounded() {
    let mut s = translator_world();
    let gross = s.m().meter.total.gross;
    let n = agent_core::meter::MAX_DAYS + 20;
    for _ in 0..n {
        s.advance(DAY);
        s.pay(PAYERS[0], USD);
    }
    assert_eq!(s.m().meter.days_total(), agent_core::meter::MAX_DAYS);
    assert_eq!(s.m().meter.total.gross, gross + n as u128 * USD);
    let page = s.c.get_earnings_days(Some(0), Some(100));
    assert_eq!(page.len(), 100);
    assert!(page.windows(2).all(|w| w[0].day < w[1].day));
}

/// Confirm every pending key deletion (the `on_keys_deleted` callback).
fn settle_deletions(s: &mut Sim) {
    let keys: Vec<String> = s.m().retiring.iter().cloned().collect();
    if !keys.is_empty() {
        s.callback(true).on_keys_deleted(keys);
    }
}

/// A47 (PC follow-up: the brick came at 586 entries whatever the name
/// length, so entries, not bytes). Every table outsiders or a long life can
/// grow is built past its cap or far past 586 entries, and the serialized
/// state blob stays a few KB: payout destinations (3,000), claimable
/// balances (700), past owners (600), finalized sales (40), payers (done
/// above) and day rows (done above).
#[test]
fn a47_every_growable_table_stays_out_of_the_blob() {
    const SMALL: u64 = 10_000;
    let mut s = translator_world();
    let start = s.c.state_bytes();
    let check = |s: &Sim, what: &str| {
        let b = s.c.state_bytes();
        assert!(
            b < SMALL,
            "{what}: state blob {b} bytes (started at {start})"
        );
    };
    // Payout destinations: 1,000 per owner, 3,000 over three owners.
    let owners = [ALICE, BOB, CAROL];
    for (k, o) in owners.iter().enumerate() {
        if k > 0 {
            let prev = owners[k - 1];
            s.by_1y(prev)
                .nft_transfer(acc(o), DEED_ID.into(), None, None);
            settle_deletions(&mut s);
            s.pay(PAYERS[0], 2_000 * USD);
        }
        for i in 0..agent_core::MAX_PAID_TO {
            s.by_1y(o)
                .withdraw(U128(1), Some(acc(&format!("dest-{k}-{i}.testnet"))))
                .detach();
        }
    }
    assert_eq!(s.m().paid_to_total(), 3_000);
    check(&s, "paid_to");
    // Claimable balances: FAULT INJECTION, 700 failed claim payouts.
    let claimers = s.m().claimable_accounts;
    for i in 0..700 {
        s.callback(false).on_transfer_out(
            OutKind::Claim,
            acc(&format!("claimer-{i}.testnet")),
            U128(1),
            None,
        );
    }
    assert_eq!(s.m().claimable_accounts, claimers + 700);
    assert_eq!(s.c.get_claimable(acc("claimer-699.testnet")).0, 1);
    check(&s, "claimable");
    // Past owners: 600 more deed moves.
    let mut owner = CAROL.to_string();
    for i in 0..600 {
        let next = format!("owner-{i}.testnet");
        s.by_1y(&owner)
            .nft_transfer(acc(&next), DEED_ID.into(), None, None);
        settle_deletions(&mut s);
        owner = next;
    }
    assert!(s.m().past_owners >= 600);
    assert!(s.m().is_past_owner("owner-0.testnet"));
    assert!(s.m().flags_for("sub.owner-0.testnet").seller_subaccount);
    check(&s, "past_owners");
    // Finalized sales: 40 list / buy / finalize cycles.
    let pair = [(BOB, BOB_RUNNER), (CAROL, CAROL_RUNNER)];
    for i in 0..40 {
        let (buyer, pk) = pair[i % 2];
        let seller = s.m().owner.clone();
        if s.m().known_keys.is_empty() {
            s.by_1y(&seller).set_runner_key(key(9)).detach();
            s.runner_key_cb(true, vec![], vec![], key_str(9));
        }
        s.by_1y(&seller)
            .list_for_sale(U128(10 * USD), CODE_HASH.into(), None);
        assert_eq!(
            s.buy(buyer, 10 * USD, pk),
            None,
            "{i}: {:?}",
            events("refused")
        );
        let id = s.m().pending.as_ref().unwrap().sale_id;
        assert_eq!(s.callback(true).on_handover(id).0, 0);
        s.advance(31 * DAY);
        s.by(MALLORY).finalize_earnout(id);
    }
    assert_eq!(s.m().closed_sales, 40);
    assert!(s.m().sales.is_empty());
    assert_eq!(s.c.get_sales(None, None).len(), 20);
    check(&s, "sales");
    // Every view still answers.
    let st = s.by(MALLORY).get_status();
    assert_eq!(st["closed_sales"], 40);
    let _ = s.c.get_earnings();
    let _ = s.c.nft_token(DEED_ID.into());
}

/// A48 (real-node review): a deed move outside `buy` (here by an approved
/// account) leaves the old owner's unwithdrawn revenue claimable by the old
/// owner, as a sale does (A29). The receiver cannot withdraw it.
#[test]
fn a48_deed_move_keeps_old_owners_revenue_theirs() {
    let mut s = translator_world();
    let free = s.m().owner_free;
    assert!(free > 0);
    let _ = s
        .by_milli(ALICE, 10)
        .nft_approve(DEED_ID.into(), acc(MALLORY), None);
    s.by_1y(MALLORY)
        .nft_transfer(acc(MALLORY), DEED_ID.into(), Some(0), None);
    assert_eq!(s.m().owner, MALLORY);
    assert_eq!(s.m().owner_free, 0, "the receiver gets no revenue");
    assert_eq!(s.c.get_claimable(acc(ALICE)).0, free);
    assert!(s.m().solvent());
    assert_eq!(
        panic_text(|| {
            s.by_1y(MALLORY).withdraw(U128(1), None).detach();
        }),
        "InsufficientFunds"
    );
}
