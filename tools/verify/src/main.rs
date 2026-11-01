//! `make verify`: replay the fixed world (SCENARIOS.md) through the real
//! agent contract in near-sdk's mocked runtime, fully offline, and print the
//! numbers and checks as JSON (`verify-output.v1`).
//!
//! Usage: verify [--json] [--out PATH] [--golden PATH]
//!   --json    print JSON (default: a human table)
//!   --out     also write the JSON to PATH
//!   --golden  compare with PATH; exit 1 on any difference
//!
//! What this does NOT cover (needs near-sandbox or testnet): real
//! `AccessKeyNotFound` errors, real token refunds, gas. Those are listed in
//! the output under `not_covered_here`.

#![allow(unused_must_use)]

use agent_core::{Status, USD};
use agent_exit::sim::*;
use agent_exit::{OutKind, DEED_ID};
use near_sdk::json_types::U128;
use near_sdk::NearToken;
use serde_json::{json, Value};
use std::panic::{catch_unwind, AssertUnwindSafe};

fn usd(a: u128) -> String {
    format!("{}.{:02}", a / USD, (a % USD) / (USD / 100))
}

struct Report {
    checks: Vec<Value>,
}

impl Report {
    fn check(&mut self, id: &str, what: &str, pass: bool, detail: Value) {
        self.checks
            .push(json!({"id": id, "what": what, "pass": pass, "detail": detail}));
    }
}

/// Run `f`; true if it panicked with a message containing `needle`.
fn panics_with(needle: &str, f: impl FnOnce()) -> bool {
    match catch_unwind(AssertUnwindSafe(f)) {
        Ok(()) => false,
        Err(e) => {
            let msg = e
                .downcast_ref::<String>()
                .cloned()
                .or_else(|| e.downcast_ref::<&str>().map(|s| s.to_string()))
                .unwrap_or_default();
            msg.contains(needle)
        }
    }
}

fn earnings_json(e: &agent_core::Earnings) -> Value {
    json!({
        "gross": usd(e.gross), "counted": usd(e.counted), "verified": usd(e.verified),
        "flagged": usd(e.flagged), "flagged_self": usd(e.flagged_self),
        "flagged_seller_subaccount": usd(e.flagged_seller_subaccount),
        "flagged_round_trip": usd(e.flagged_round_trip),
        "payments": e.payments, "counted_payers": e.counted_payers, "days": e.days.len(),
    })
}

fn claim(s: &Sim, who: &str) -> u128 {
    s.c.get_claimable(acc(who)).0
}

fn run() -> Value {
    let mut r = Report { checks: vec![] };

    // S2 / S4: the meter on the fixed history.
    let t = translator_world().c.get_earnings();
    let c = cheat_world().c.get_earnings();
    r.check(
        "S2",
        "meter counts by payer and day: gross 500.00, counted 360.00, verified 120.00, flagged 140.00",
        t.gross == 500 * USD
            && t.counted == 360 * USD
            && t.verified == 120 * USD
            && t.flagged_self == 100 * USD
            && t.flagged_seller_subaccount == 40 * USD
            && t.counted_payers == 6
            && t.days.len() == 6,
        earnings_json(&t),
    );
    r.check(
        "S4",
        "both earn 500.00: translator counted 360.00 vs cheat counted 0.00",
        t.counted == 360 * USD && c.gross == 500 * USD && c.counted == 0,
        json!({"translator": usd(t.counted), "cheat": usd(c.counted)}),
    );

    // S1: listing.
    let mut s = translator_world();
    let card = s
        .by(ALICE)
        .list_for_sale(U128(1_000 * USD), CODE_HASH.into(), None);
    r.check(
        "S1",
        "list: listing_version 1, price 1000.00 = 700.00 now + 300.00 escrow",
        card["listing_version"] == 1 && card["paid_now"] == "700000000",
        json!({"listing_version": card["listing_version"], "known_keys": card["known_keys"]}),
    );

    // S5: one-step buy.
    let keys_before = s.c.known_keys();
    let in_flight = s.buy(BOB, 1_000 * USD, BOB_RUNNER).is_none();
    let batch = key_actions(AGENT);
    let unused = s.callback(true).on_handover(1).0;
    let payouts = ft_transfers();
    let keys_after = s.c.known_keys();
    let owner = s.c.nft_token(DEED_ID.into()).unwrap().owner_id.to_string();
    r.check(
        "S5",
        "one buy: deed to bob, alice's runner key deleted, bob's added, alice 700.00, escrow 300.00, epoch 2",
        in_flight
            && batch.len() == 2
            && unused == 0
            && payouts == vec![(ALICE.to_string(), 700 * USD)]
            && owner == BOB
            && keys_after == vec![key_str(BOB_RUNNER)]
            && s.m().owner_epoch == 2
            && s.m().escrow_left() == 300 * USD
            && s.m().status == Status::Sold,
        json!({
            "deed_owner": owner, "keys_before": keys_before, "keys_after": keys_after,
            "key_batch": batch, "seller_paid": usd(payouts.first().map(|p| p.1).unwrap_or(0)),
            "escrow": usd(s.m().escrow_left()), "owner_epoch": s.m().owner_epoch,
        }),
    );

    // E1 / E2 / E3 / E7 / E10: refusals return the full amount.
    let l = listed_translator();
    let (v, h) = (l.m().listing_version, l.c.known_keys_hash());
    let cases = [
        (
            "E1",
            "stale listing_version refused, full refund",
            Sim::buy_msg(v - 1, &h, BOB_RUNNER),
            1_000 * USD,
            300,
        ),
        (
            "E2",
            "wrong known_keys_hash refused, full refund",
            Sim::buy_msg(v, "00", BOB_RUNNER),
            1_000 * USD,
            300,
        ),
        (
            "E3",
            "underpay 999.99 refused, full refund",
            Sim::buy_msg(v, &h, BOB_RUNNER),
            999_990_000,
            300,
        ),
        (
            "E7",
            "too little gas refused before any state change",
            Sim::buy_msg(v, &h, BOB_RUNNER),
            1_000 * USD,
            50,
        ),
        (
            "E10",
            "malformed msg refused, full refund",
            "{\"kind\":".to_string(),
            7 * USD,
            300,
        ),
    ];
    for (id, what, msg, amount, gas) in cases {
        let mut s = listed_translator();
        let refunded = s.ft(BOB, amount, &msg, gas) == Some(amount);
        let unchanged = s.m().status == Status::Listed;
        r.check(
            id,
            what,
            refunded && unchanged,
            json!({"refunded": refunded}),
        );
    }
    let mut s = listed_translator();
    s.buy(BOB, 1_000 * USD, BOB_RUNNER);
    let second = s.buy(CAROL, 1_000 * USD, CAROL_RUNNER);
    r.check(
        "E4",
        "second buy while settling refused, full refund",
        second == Some(1_000 * USD),
        json!({"refunded": usd(second.unwrap_or(0))}),
    );
    let mut s = listed_translator();
    s.buy(BOB, 1_000_500_000, BOB_RUNNER);
    let change = s.callback(true).on_handover(1).0;
    r.check(
        "E3",
        "overpay 1000.50: 0.50 returned",
        change == 500_000,
        json!({"returned": usd(change)}),
    );

    // E5: rollback.
    let mut s = listed_translator();
    let before = s.m().clone();
    s.buy(BOB, 1_000 * USD, BOB_RUNNER);
    let unused = s.callback(false).on_handover(1).0;
    let mut expect = before;
    expect.next_sale_id = 2;
    r.check(
        "E5",
        "key batch fails: state restored, bob refunded 1000.00, no payout",
        unused == 1_000 * USD && ft_transfers().is_empty() && s.m() == &expect,
        json!({"refunded": usd(unused), "status": s.m().status, "keys": s.c.known_keys()}),
    );

    // E6: payout falls back to claimable.
    let mut s = listed_translator();
    s.buy(BOB, 1_000 * USD, BOB_RUNNER);
    s.callback(true).on_handover(1);
    s.callback(false)
        .on_transfer_out(OutKind::SellerPayout, acc(ALICE), U128(700 * USD));
    let cl = claim(&s, ALICE);
    r.check(
        "E6",
        "seller payout fails: 700.00 claimable, solvency holds",
        cl == 700 * USD && s.m().solvent(),
        json!({"claimable": usd(cl)}),
    );

    // E8 / A14: fake token.
    let mut s = translator_world();
    let fake = panics_with("UnknownToken", || {
        s.by(MALLORY)
            .ft_on_transfer(acc(MALLORY), U128(USD), "".into());
    });
    r.check(
        "E8",
        "ft_on_transfer from a non-token panics UnknownToken",
        fake,
        json!({}),
    );
    r.check("A14", "fake token cannot record a payment", fake, json!({}));

    // E15: dust.
    let mut s = translator_world();
    let dust = s.pay("dust.testnet", 50_000) == Some(50_000);
    r.check(
        "E15",
        "0.05 payment refunded, no payer entry",
        dust,
        json!({}),
    );

    // E17: storage reserve.
    let mut s = translator_world();
    let max = s.by_1y(ALICE).near_withdrawable();
    let over = panics_with("StorageReserve", || {
        s.by_1y(ALICE)
            .withdraw_near(max.saturating_add(NearToken::from_yoctonear(1)));
    });
    let exact = catch_unwind(AssertUnwindSafe(|| {
        s.by_1y(ALICE).withdraw_near(max);
    }))
    .is_ok();
    r.check(
        "E17",
        "owner withdraws everything above the reserve; one yocto more is refused",
        over && exact,
        json!({}),
    );

    // Earn-out: S7, S8, E21, S9, S10.
    let mut s = sold_translator();
    for i in 1..=4u64 {
        s.now = T0 + (10 + i) * DAY;
        s.pay(PAYERS[0], 25 * USD);
    }
    let mid = claim(&s, ALICE);
    s.now = T0 + 41 * DAY;
    s.by(MALLORY).finalize_earnout(1);
    let (a7, b7) = (claim(&s, ALICE), claim(&s, BOB));
    r.check(
        "S7",
        "4 x 25.00 after sale release 50.00; floor tops up 10.00; alice 60.00, bob 240.00",
        mid == 50 * USD && a7 == 60 * USD && b7 == 240 * USD,
        json!({"released_before_end": usd(mid), "alice": usd(a7), "bob": usd(b7)}),
    );
    let mut s = sold_translator();
    s.now = T0 + 41 * DAY;
    let f1 = s.by(MALLORY).finalize_earnout(1);
    let f2 = s.by(CAROL).finalize_earnout(1);
    let (a8, b8) = (claim(&s, ALICE), claim(&s, BOB));
    r.check(
        "S8",
        "silence pays the floor: alice 60.00, bob 240.00",
        a8 == 60 * USD && b8 == 240 * USD,
        json!({"alice": usd(a8), "bob": usd(b8)}),
    );
    r.check(
        "E21",
        "finalize twice, by anyone: identical result",
        f1 == f2,
        json!({}),
    );
    let mut s = sold_translator();
    for i in 0..24u64 {
        s.now = T0 + 10 * DAY + (i + 1) * 3_600 * agent_core::NANOS_PER_SEC;
        s.pay(PAYERS[(i % 6) as usize], 25 * USD);
    }
    s.now = T0 + 41 * DAY;
    s.by(MALLORY).finalize_earnout(1);
    let (a9, b9) = (claim(&s, ALICE), claim(&s, BOB));
    r.check(
        "S9",
        "600.00 counted after sale: release caps at 300.00, nothing back to bob",
        a9 == 300 * USD && b9 == 0,
        json!({"alice": usd(a9), "bob": usd(b9)}),
    );
    let mut s = sold_translator();
    s.pay(ALICE, 100 * USD);
    let wash = claim(&s, ALICE);
    r.check(
        "S10",
        "alice pays 100.00 after the sale: flagged self, releases 0.00",
        wash == 0 && events("payment")[0]["flags"]["self"] == true,
        json!({"released": usd(wash)}),
    );

    // A10-A12: wash flags.
    let mut s = translator_world();
    s.by_1y(ALICE)
        .withdraw(U128(10 * USD), Some(acc("p7.testnet")));
    s.callback(true)
        .on_transfer_out(OutKind::OwnerWithdraw, acc("p7.testnet"), U128(10 * USD));
    s.pay("p7.testnet", 10 * USD);
    let rt = events("payment")[0]["flags"]["round_trip"] == true;
    s.pay(ALICE, 5 * USD);
    let sf = events("payment")[0]["flags"]["self"] == true;
    s.pay(X_ALICE, 5 * USD);
    let sb = events("payment")[0]["flags"]["seller_subaccount"] == true;
    r.check("A10", "self-payment flagged, not counted", sf, json!({}));
    r.check("A11", "seller sub-account payment flagged", sb, json!({}));
    r.check(
        "A12",
        "round trip (agent paid P, P pays back) flagged",
        rt,
        json!({}),
    );

    // A16 / E24: deed moved outside buy().
    let mut s = translator_world();
    s.by_1y(ALICE)
        .nft_transfer(acc(MALLORY), DEED_ID.into(), None, None);
    let del = key_actions(AGENT);
    r.check(
        "A16",
        "deed moved outside buy(): runner keys deleted, agent paused",
        del == vec![("delete".to_string(), key_str(ALICE_RUNNER))] && s.m().paused,
        json!({"deleted": del}),
    );

    // Break-in by the old seller after the sale (contract part).
    let mut s = sold_translator();
    let a19 = panics_with("NotOwner", || {
        s.by_1y(ALICE).withdraw(U128(USD), None);
    });
    let a22 = panics_with("NotOwner", || {
        s.by_1y(ALICE)
            .nft_transfer(acc(ALICE), DEED_ID.into(), None, None);
    });
    let a17 = !s.c.known_keys().contains(&key_str(ALICE_RUNNER));
    r.check(
        "A17",
        "old runner key no longer known to the contract (live AccessKeyNotFound: sandbox/testnet)",
        a17,
        json!({}),
    );
    r.check(
        "A19",
        "old seller withdraw refused NotOwner",
        a19,
        json!({}),
    );
    r.check(
        "A22",
        "old seller moves the deed back: NotOwner",
        a22,
        json!({}),
    );

    // A23: hidden key.
    let mut s = sold_translator();
    s.now = T0 + 11 * DAY;
    s.pay(PAYERS[0], 20 * USD);
    let resp = s.runner(key(HIDDEN_KEY)).heartbeat();
    let back = claim(&s, BOB);
    r.check(
        "A23",
        "self-call signed by a key the contract never added: frozen, 290.00 escrow back to bob",
        resp["frozen"] == true && back == 290 * USD && s.m().frozen,
        json!({"returned_to_buyer": usd(back)}),
    );

    // S15: resale.
    let mut s = sold_translator();
    s.by(BOB)
        .list_for_sale(U128(2_000 * USD), CODE_HASH.into(), None);
    s.buy(CAROL, 2_000 * USD, CAROL_RUNNER);
    s.callback(true).on_handover(2);
    s.now = T0 + 12 * DAY;
    s.pay(PAYERS[2], 10 * USD);
    let first = events("earnout_release")
        .first()
        .map(|e| e["seller"] == ALICE)
        .unwrap_or(false);
    r.check(
        "S15",
        "resale to carol: history travels, alice's earn-out keeps releasing",
        s.m().owner == CAROL && s.c.get_earnings().counted == 370 * USD && first,
        json!({"owner": s.m().owner, "owner_epoch": s.m().owner_epoch}),
    );

    // NEAR Verified: S3, E11-E14, A13 (signatures are TEST FIXTURES made
    // with local keys; the real NEAR AI signer is gate G6).
    let note = || {
        events("payment")[0]["verify"]
            .as_str()
            .unwrap_or("")
            .to_string()
    };
    let mut s = Sim::new(AGENT, ALICE);
    s.ft(PAYERS[0], 12 * USD, &job_msg(&job(0)), 100);
    let ok = events("payment")[0]["verified"] == true;
    s.ft(
        PAYERS[1],
        12 * USD,
        r#"{"kind":"pay","job_id":"job-2"}"#,
        100,
    );
    let plain =
        events("payment")[0]["verified"] == false && events("payment")[0]["counted"] == true;
    r.check(
        "S3",
        "payment with an allowlisted signature over model:sha256(request):sha256(response) is verified; without one it counts, not verified",
        ok && plain,
        json!({"signer": test_signer(), "fixture": "fixtures/test-signatures/translator.json (TEST keys)"}),
    );
    s.ft(PAYERS[2], 12 * USD, &job_msg(&job(0)), 100);
    let e11 = note();
    r.check(
        "E11",
        "same signature twice: second counted, not verified (SignatureUsed)",
        e11 == "SignatureUsed",
        json!({"reason": e11}),
    );
    s.ft(
        PAYERS[2],
        12 * USD,
        &job_msg(&fixtures()["wrong_agent"]),
        100,
    );
    let e12 = note();
    r.check(
        "E12",
        "signature for a request naming another agent: not verified (WrongAgent)",
        e12 == "WrongAgent",
        json!({"reason": e12}),
    );
    s.ft(PAYERS[2], 12 * USD, &job_msg(&fixtures()["rogue"]), 100);
    let e13 = note();
    let mut forged = job(1);
    forged["response_sha256"] = json!("0".repeat(64));
    s.ft(PAYERS[2], 12 * USD, &job_msg(&forged), 100);
    let e13b = note();
    r.check(
        "E13",
        "unknown signer and tampered response: not verified, no panic",
        e13 == "UnknownSigner" && e13b == "UnknownSigner",
        json!({"reasons": [e13, e13b]}),
    );
    r.check(
        "A13",
        "replayed or forged NEAR AI signature never verifies",
        e11 == "SignatureUsed" && e13b == "UnknownSigner",
        json!({}),
    );
    let rogue = fixtures()["rogue_signer"]
        .as_str()
        .unwrap_or("")
        .to_string();
    s.by(ADMIN).add_signer(rogue);
    s.by(ADMIN).remove_signer(test_signer());
    let kept = s.c.get_earnings().verified;
    s.ft(PAYERS[3], 12 * USD, &job_msg(&job(2)), 100);
    let e14 = note();
    r.check(
        "E14",
        "signer rotated: old badges stay, removed signer no longer verifies",
        kept == 12 * USD && e14 == "UnknownSigner",
        json!({"verified_kept": usd(kept), "reason": e14}),
    );

    // S19: dynamic deed metadata.
    let s = translator_world();
    let tok = s.c.nft_token(DEED_ID.into()).unwrap();
    let extra: Value = serde_json::from_str(&tok.metadata.unwrap().extra.unwrap()).unwrap();
    r.check(
        "S19",
        "deed metadata extra carries meter totals (static part)",
        extra["earnings"]["counted"] == "360000000",
        json!({"extra_schema": extra["schema"]}),
    );

    // Feature 12: spending policy enforced by the contract (3 demo beats).
    let mut s = sold_translator();
    let free0 = s.m().owner_free;
    let rel0 = claim(&s, ALICE);
    s.by_1y(BOB).set_limits(demo_limits());
    s.now += 3_600 * agent_core::NANOS_PER_SEC;
    s.ft(
        PAYERS[0],
        25 * USD,
        r#"{"kind":"pay","job_id":"job-b1"}"#,
        100,
    );
    let (to_owner, to_treasury) = (s.m().owner_free - free0, s.m().treasury);
    let release = claim(&s, ALICE) - rel0;
    s.ft(
        PAYERS[1],
        25 * USD,
        r#"{"kind":"pay","job_id":"job-b2"}"#,
        100,
    );
    let paid = s
        .runner(key(BOB_RUNNER))
        .pay(acc(SUMMARIZER), U128(8 * USD));
    r.check(
        "S11",
        "limits set; a 25.00 job splits 20.00 owner / 5.00 treasury (seller's share = earn-out release 12.50); runner pays summarizer 8.00",
        to_owner == 20 * USD
            && to_treasury == 5 * USD
            && release == 12_500_000
            && paid["ok"] == true
            && ft_transfers() == vec![(SUMMARIZER.to_string(), 8 * USD)]
            && s.m().solvent(),
        json!({"to_owner": usd(to_owner), "to_treasury": usd(to_treasury),
               "seller_earnout_release": usd(release), "paid_summarizer": "8.00",
               "treasury_left": usd(s.m().treasury)}),
    );
    let before = s.m().clone();
    let over = panics_with(
        "PolicyViolation:DailyCap: 8.00 + 45.00 > daily cap 50.00",
        || {
            s.runner(key(BOB_RUNNER))
                .pay(acc(SUMMARIZER), U128(45 * USD));
        },
    );
    let payee = panics_with("PolicyViolation:Payee", || {
        s.runner(key(BOB_RUNNER)).pay(acc(MALLORY), U128(USD));
    });
    r.check(
        "S12",
        "over-limit payment fails: PolicyViolation:DailyCap (8.00 + 45.00 > 50.00); a payee not on the list fails; balances unchanged",
        over && payee && s.m() == &before,
        json!({"error": "PolicyViolation:DailyCap: 8.00 + 45.00 > daily cap 50.00"}),
    );
    let alice_no = panics_with("NotOwner", || {
        s.by_1y(ALICE).set_limits(demo_limits());
    });
    let runner_no = panics_with("NotOwner", || {
        let mut c = s.call(AGENT);
        c.signer_pk = key(BOB_RUNNER);
        c.deposit = NearToken::from_yoctonear(1);
        s.env(&c);
        s.c.set_limits(demo_limits());
    });
    r.check(
        "S13",
        "only the owner sets limits: bob applies, the old seller and the runner get NotOwner",
        alice_no && runner_no && s.m().limits.daily_cap == 50 * USD,
        json!({}),
    );

    // E18 / A15: the seller burns the cap just before the sale.
    let mut s = translator_world();
    s.by_1y(ALICE).set_limits(demo_limits());
    for p in PAYERS.iter().take(5) {
        s.pay(p, 60 * USD);
    }
    s.runner(key(ALICE_RUNNER))
        .pay(acc(SUMMARIZER), U128(50 * USD));
    s.by(ALICE)
        .list_for_sale(U128(1_000 * USD), CODE_HASH.into(), None);
    s.buy(BOB, 1_000 * USD, BOB_RUNNER);
    let id = s.m().pending.as_ref().map(|p| p.sale_id).unwrap_or(0);
    s.callback(true).on_handover(id);
    let left = s.by(BOB).get_limits()["cap_left_today"].clone();
    r.check(
        "E18",
        "cap resets at sale: same day, bob's epoch starts with the full 50.00",
        left == "50000000",
        json!({"cap_left_today": left}),
    );
    r.check(
        "A15",
        "seller burns the daily cap before the sale: buyer's cap unaffected",
        left == "50000000",
        json!({}),
    );

    // E19: UTC day boundary.
    let mut s = business_world();
    s.ft(PAYERS[2], 300 * USD, "", 100);
    s.runner(key(BOB_RUNNER))
        .pay(acc(SUMMARIZER), U128(50 * USD));
    let capped = panics_with("PolicyViolation:DailyCap", || {
        s.runner(key(BOB_RUNNER)).pay(acc(SUMMARIZER), U128(USD));
    });
    s.now = (s.now / DAY + 1) * DAY;
    let next = s
        .runner(key(BOB_RUNNER))
        .pay(acc(SUMMARIZER), U128(20 * USD));
    r.check(
        "E19",
        "spending cap resets by block-timestamp UTC day",
        capped && next["ok"] == true,
        json!({}),
    );

    // E33: bad limits.
    let mut s = sold_translator();
    let mut split = demo_limits();
    split.split.owner_bps = 7_000;
    let mut cap = demo_limits();
    cap.daily_cap = U128(1_000_001 * USD);
    let mut selfpay = demo_limits();
    selfpay.allowed_payees = vec![acc(AGENT)];
    let all_bad = [split, cap, selfpay].into_iter().all(|l| {
        panics_with("BadLimits", || {
            s.by_1y(BOB).set_limits(l);
        })
    });
    let negative = serde_json::from_str::<agent_exit::LimitsArgs>(
        r#"{"price_floor":"-5","daily_cap":"50","allowed_payees":[],"split":{"owner_bps":10000,"treasury_bps":0}}"#,
    )
    .is_err();
    r.check(
        "E33",
        "set_limits with split != 100%, an absurd cap, the agent as payee or a negative amount: refused",
        all_bad && negative,
        json!({}),
    );

    // A24: the seller drains the treasury while listed.
    let mut s = translator_world();
    s.by_1y(ALICE).set_limits(demo_limits());
    for p in PAYERS.iter().take(5) {
        s.pay(p, 60 * USD);
    }
    let card = s
        .by(ALICE)
        .list_for_sale(U128(1_000 * USD), CODE_HASH.into(), None);
    let paused = panics_with("Listed: spending paused", || {
        s.runner(key(ALICE_RUNNER))
            .pay(acc(SUMMARIZER), U128(10 * USD));
    });
    let kept = s.m().treasury;
    s.inject_treasury_drain(10 * USD);
    let stale = s.buy(BOB, 1_000 * USD, BOB_RUNNER) == Some(1_000 * USD)
        && events("refused")
            .iter()
            .any(|e| e["reason"] == "TreasuryChanged");
    r.check(
        "A24",
        "seller drains the treasury while listed: pay fails 'Listed: spending paused'; buy refuses (full refund) if the treasury is below the listing's snapshot",
        paused && kept == 60 * USD && card["treasury_snapshot"] == "60000000" && stale,
        json!({"treasury_snapshot": card["treasury_snapshot"], "error": "Listed: spending paused", "stale_buy": "TreasuryChanged"}),
    );

    let passed = r.checks.iter().filter(|c| c["pass"] == true).count();
    let failed = r.checks.len() - passed;
    json!({
        "schema": "verify-output.v1",
        "project": "agent-exit",
        "money": "xUSD-FAKE, a fake test dollar with no value (6 decimals)",
        "runtime": "near-sdk 5.30 mocked runtime, offline",
        "world": {
            "translator": earnings_json(&t),
            "cheat": earnings_json(&c),
            "listing": {"price": "1000.00", "earnout": "30%", "release_share": "50%",
                        "floor": "20% of escrow", "window_days": 30},
        },
        "checks": r.checks,
        "summary": {"passed": passed, "failed": failed},
        "not_covered_here": [
            {"id": "S3,G6", "part": "the real NEAR AI Cloud signer (fixtures here are signed with local test keys)",
             "where": "owner's machine, day-1 gate G6"},
            {"id": "S5,E5,E6,A17,A21", "part": "real key errors, real token refunds, gas",
             "where": "near-sandbox (make test-sandbox) or testnet"},
            {"id": "S6,A18,A20", "part": "Chain Signatures Base wallet and pay_base",
             "where": "batch B3"},
            {"id": "S11,S12", "part": "the same limits on a real node (sandbox/testnet)",
             "where": "near-sandbox or testnet"},
        ],
    })
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let flag = |f: &str| args.iter().any(|a| a == f);
    let opt = |f: &str| {
        args.iter()
            .position(|a| a == f)
            .and_then(|i| args.get(i + 1))
            .cloned()
    };
    // Expected panics are part of the checks; keep stderr clean.
    std::panic::set_hook(Box::new(|_| {}));
    let out = run();
    let _ = std::panic::take_hook();
    let text = serde_json::to_string_pretty(&out).expect("json") + "\n";

    if flag("--json") {
        print!("{text}");
    } else {
        for c in out["checks"].as_array().expect("checks") {
            println!(
                "{} {:5} {}",
                if c["pass"] == true { "PASS" } else { "FAIL" },
                c["id"].as_str().unwrap_or(""),
                c["what"].as_str().unwrap_or("")
            );
        }
        println!("summary: {}", out["summary"]);
    }
    if let Some(p) = opt("--out") {
        std::fs::write(&p, &text).expect("write --out");
        eprintln!("verify: wrote {p}");
    }
    let mut ok = out["summary"]["failed"] == 0;
    if !ok {
        eprintln!("verify: some checks FAILED");
    }
    if let Some(g) = opt("--golden") {
        let golden: Value = serde_json::from_str(
            &std::fs::read_to_string(&g).unwrap_or_else(|e| panic!("read {g}: {e}")),
        )
        .expect("golden is JSON");
        if golden == out {
            eprintln!("verify: output equals {g}");
        } else {
            ok = false;
            eprintln!("verify: output differs from {g}:");
            diff("", &golden, &out);
        }
    }
    std::process::exit(if ok { 0 } else { 1 });
}

fn diff(path: &str, a: &Value, b: &Value) {
    match (a, b) {
        (Value::Object(x), Value::Object(y)) => {
            for k in x.keys().chain(y.keys().filter(|k| !x.contains_key(*k))) {
                diff(
                    &format!("{path}.{k}"),
                    x.get(k).unwrap_or(&Value::Null),
                    y.get(k).unwrap_or(&Value::Null),
                );
            }
        }
        (Value::Array(x), Value::Array(y)) if x.len() == y.len() => {
            for (i, (p, q)) in x.iter().zip(y).enumerate() {
                diff(&format!("{path}[{i}]"), p, q);
            }
        }
        _ if a != b => eprintln!("  {path}: golden {a} != now {b}"),
        _ => {}
    }
}
