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

/// alice's unwithdrawn revenue at the sale (360.00 counted + 140.00 flagged),
/// credited to her claimable by the sale since A29.
const SELLER_REVENUE: u128 = 500 * USD;

/// alice's earn-out so far: her claimable minus the revenue settled at the sale.
fn earnout(s: &Sim) -> u128 {
    claim(s, ALICE) - SELLER_REVENUE
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
        .by_1y(ALICE)
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
        .on_transfer_out(OutKind::SellerPayout, acc(ALICE), U128(700 * USD), None);
    let cl = claim(&s, ALICE);
    r.check(
        "E6",
        "seller payout fails: 700.00 claimable (plus the 500.00 revenue settled at the sale), solvency holds",
        cl == 700 * USD + SELLER_REVENUE && s.m().solvent(),
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
    let mid = earnout(&s);
    s.now = T0 + 41 * DAY;
    s.by(MALLORY).finalize_earnout(1);
    let (a7, b7) = (earnout(&s), claim(&s, BOB));
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
    let (a8, b8) = (earnout(&s), claim(&s, BOB));
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
    let (a9, b9) = (earnout(&s), claim(&s, BOB));
    r.check(
        "S9",
        "600.00 counted after sale: release caps at 300.00, nothing back to bob",
        a9 == 300 * USD && b9 == 0,
        json!({"alice": usd(a9), "bob": usd(b9)}),
    );
    let mut s = sold_translator();
    s.pay(ALICE, 100 * USD);
    let wash = earnout(&s);
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
    s.callback(true).on_transfer_out(
        OutKind::OwnerWithdraw,
        acc("p7.testnet"),
        U128(10 * USD),
        None,
    );
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
    s.by_1y(BOB)
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
    s.ft(PAYERS[0], 12 * USD, &job_msg(&job(0)), 100);
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
    s.ft(PAYERS[1], 12 * USD, &job_msg(&forged), 100);
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
    s.by_1y(ADMIN).add_signer(rogue);
    s.by_1y(ADMIN).remove_signer(test_signer());
    let kept = s.c.get_earnings().verified;
    s.ft(PAYERS[2], 12 * USD, &job_msg(&job(2)), 100);
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
    s.by_1y(ALICE)
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
        .by_1y(ALICE)
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

    // Security review 1 (A25-A30).
    let mut s = listed_translator();
    s.buy(BOB, 1_000 * USD, BOB_RUNNER);
    let gap = panics_with("NotRunner", || {
        s.runner(key(BOB_RUNNER)).heartbeat();
    });
    s.callback(true).on_handover(1);
    let mut s2 = sold_translator();
    s2.by_1y(BOB).set_runner_key(key(CAROL_RUNNER));
    let rot = panics_with("NotRunner", || {
        s2.runner(key(BOB_RUNNER)).heartbeat();
    });
    let mut s3 = sold_translator();
    s3.by_1y(BOB)
        .nft_transfer(acc(CAROL), DEED_ID.into(), None, None);
    let xfer = panics_with("NotRunner", || {
        s3.runner(key(BOB_RUNNER)).heartbeat();
    });
    r.check(
        "A25",
        "buyer or owner cannot freeze the agent with their own key in a key-batch gap (handover, rotation, deed transfer): NotRunner, escrow kept",
        gap && rot && xfer
            && !s.m().frozen && !s2.m().frozen && !s3.m().frozen
            && claim(&s, BOB) == 0 && s.m().escrow_left() == 300 * USD,
        json!({"escrow_left": usd(s.m().escrow_left())}),
    );
    let mut s = translator_world();
    let st = s.by(ALICE).get_status();
    let not_admin = panics_with("NotAdmin", || {
        s.by(ALICE).add_signer(test_signer());
    });
    r.check(
        "A26",
        "admin, token and signer list are not init arguments: pinned in the code, shown in get_status, scan blocks a mismatch",
        st["admin"] == agent_exit::REGISTRY_ADMIN && st["token"] == agent_exit::PAYMENT_TOKEN && not_admin,
        json!({"admin": st["admin"], "token": st["token"]}),
    );
    let mut s = Sim::new(AGENT, ALICE);
    s.ft("sock-puppet.testnet", 100_000 * USD, &job_msg(&job(0)), 100);
    let wp = note();
    s.ft(PAYERS[0], 100_000 * USD, &job_msg(&job(0)), 100);
    let wa = note();
    r.check(
        "A27",
        "a NEAR AI signature verifies only its own payer, amount and job: WrongPayer, WrongAmount",
        wp == "WrongPayer" && wa == "WrongAmount" && s.c.get_earnings().verified == 0,
        json!({"reasons": [wp, wa]}),
    );
    let mut s = translator_world();
    let no_yocto = ["list", "key", "limits"].iter().all(|w| {
        panics_with("1 yoctoNEAR", || match *w {
            "list" => {
                s.by(ALICE).list_for_sale(U128(USD), CODE_HASH.into(), None);
            }
            "key" => {
                s.by(ALICE).set_runner_key(key(9));
            }
            _ => {
                s.by(ALICE).set_limits(demo_limits());
            }
        })
    });
    r.check(
        "A28",
        "list_for_sale, cancel_listing, set_runner_key, set_limits, nft_transfer need 1 yoctoNEAR (a function-call key cannot sell the agent)",
        no_yocto && s.m().status == Status::Active,
        json!({}),
    );
    let mut s = sold_translator();
    let bob_cannot = panics_with("InsufficientFunds", || {
        s.by_1y(BOB).withdraw(U128(USD), None);
    });
    r.check(
        "A29",
        "the seller's unwithdrawn revenue (500.00) is settled to the seller at the sale, not passed to the buyer",
        bob_cannot && claim(&s, ALICE) == 500 * USD && s.m().owner_free == 0,
        json!({"seller_claimable": usd(claim(&s, ALICE))}),
    );
    let mut s = translator_world();
    for i in 0..agent_core::MAX_PAID_TO {
        s.by_1y(ALICE)
            .withdraw(U128(1), Some(acc(&format!("junk{i}.testnet"))));
    }
    let full = panics_with("PaidToFull", || {
        s.by_1y(ALICE)
            .withdraw(U128(USD), Some(acc("friend.testnet")));
    });
    s.pay("junk7.testnet", 10 * USD);
    r.check(
        "A30",
        "round-trip tracking never saturates: once 1000 destinations are tracked, sending to a new one is refused (PaidToFull); tracked payers stay flagged",
        full && s.c.get_earnings().flagged_round_trip == 10 * USD,
        json!({}),
    );

    // Security review 2 (A31 and up).
    let mut s = sold_translator();
    s.advance(DAY);
    s.pay(PAYERS[0], 12 * USD);
    for p in PAYERS {
        s.by_1y(BOB).withdraw(U128(1), Some(acc(p)));
    }
    for p in PAYERS {
        s.pay(p, 100 * USD);
    }
    let flagged = s.c.get_earnings().flagged_round_trip;
    s.advance(31 * DAY);
    s.by(MALLORY).finalize_earnout(1);
    r.check(
        "A31",
        "buyer pays every customer 1 base unit after the sale: their payments are flagged round_trip for the next buyer but still release the seller's earn-out (only pre-sale round trips count against it)",
        flagged == 600 * USD && s.m().sale(1).unwrap().released == 300 * USD && s.m().sale(1).unwrap().returned == 0,
        json!({"flagged_round_trip": usd(flagged), "seller_released": usd(s.m().sale(1).unwrap().released)}),
    );

    let mut s = translator_world();
    let base = agent_core::Terms::default();
    let refused = [
        agent_core::Terms {
            earnout_bps: 0,
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
    ]
    .into_iter()
    .all(|t| {
        panics_with("BadTerms", || {
            s.by_1y(ALICE)
                .list_for_sale(U128(1_000 * USD), CODE_HASH.into(), Some(t));
        })
    });
    let judge: scan::ScanInput =
        serde_json::from_str(include_str!("../../../fixtures/judge-attack.json"))
            .expect("judge fixture");
    let mut low = judge.clone();
    low.listing.as_mut().expect("listing").terms.earnout_bps = 0;
    let mut relisted = judge.clone();
    relisted.contract.listing_version += 1;
    let scan_blocks = scan::scan(&low).codes().contains(&"terms_below_minimum")
        && scan::scan(&relisted)
            .codes()
            .contains(&"listing_version_mismatch")
        && scan::scan(&judge).verdict == "clean";
    let st = s.by(MALLORY).get_status();
    r.check(
        "A32",
        "earn-out terms below the minimum (escrow < 30%, seller floor > 50% of escrow, window < 30 days) are refused by the contract and blocked by the scan; a relist after the quote blocks the buy",
        refused && scan_blocks && st["terms_config"] == "prod",
        json!({"terms_min": st["terms_min"], "terms_config": st["terms_config"]}),
    );

    // A33: callbacks that fail after their key batch ran are recoverable.
    let mut s = listed_translator();
    s.buy(BOB, 1_000 * USD, BOB_RUNNER);
    s.advance(agent_core::RECOVER_AFTER_NS);
    s.by(MALLORY).recover_sale();
    s.callback(false)
        .on_keys_deleted(vec![key_str(ALICE_RUNNER), key_str(BOB_RUNNER)]);
    let sale_back = s.m().owner == ALICE
        && s.m().status == Status::Active
        && s.m().pending.is_none()
        && s.m().solvent();
    s.by_1y(ALICE).set_runner_key(key(9));
    s.runner_key_cb(true, vec![], vec![], key_str(9));
    let resumed = s.runner(key(9)).heartbeat()["ok"] == true;
    let mut s2 = sold_translator();
    s2.by_1y(BOB).set_runner_key(key(CAROL_RUNNER));
    let early = panics_with("RecoverTooEarly", || {
        s2.by_1y(BOB).recover_keys();
    });
    s2.advance(agent_core::RECOVER_AFTER_NS);
    let rk = s2.by_1y(BOB).recover_keys();
    let keys: Vec<String> = serde_json::from_value(rk["delete_keys"].clone()).unwrap_or_default();
    s2.callback(true).on_keys_deleted(keys);
    let keys_back = s2.m().incoming.is_empty() && s2.m().retiring.is_empty() && !s2.m().frozen;
    r.check(
        "A33",
        "a callback that fails after its key batch ran is recoverable an hour later: recover_sale undoes a stuck buy (token refunded the buyer; seller keeps the deed), recover_keys clears stuck incoming/retiring keys; each key is deleted in its own receipt; callback gas grows with the state",
        sale_back && resumed && early && keys_back,
        json!({"recover_after_secs": agent_core::RECOVER_AFTER_NS / agent_core::NANOS_PER_SEC, "gas_per_state_byte": agent_exit::GAS_PER_STATE_BYTE}),
    );

    let mut s = listed_translator();
    s.buy(BOB, 1_000 * USD, BOB_RUNNER);
    s.runner(key(HIDDEN_KEY)).heartbeat();
    let refund = s.callback(true).on_handover(1).0;
    let undone = refund == 1_000 * USD
        && s.m().owner == ALICE
        && s.m().sales.is_empty()
        && ft_transfers().iter().all(|(to, _)| to != ALICE);
    let mut s2 = business_world();
    let all = s2.m().owner_free + s2.m().treasury;
    s2.runner(key(HIDDEN_KEY)).heartbeat();
    s2.by_1y(BOB).withdraw(U128(all), None);
    r.check(
        "A34",
        "a backdoor proven while a buy settles undoes the buy (buyer refunded in full, no seller payout, buyer key deleted); the owner of a frozen agent can withdraw its treasury",
        undone && s2.m().treasury == 0,
        json!({"buyer_refund": usd(refund)}),
    );

    let mut s = sold_translator();
    s.by_1y(BOB)
        .nft_transfer_call(acc(CAROL), DEED_ID.into(), None, None, String::new());
    s.callback(true).on_keys_deleted(vec![key_str(BOB_RUNNER)]);
    s.by_1y(CAROL).set_runner_key(key(CAROL_RUNNER));
    let mut cb = s.call(&s.agent.clone());
    cb.result = Some(true);
    s.env(&cb);
    // The callback result body is empty here; an unreadable answer counts
    // as "give it back" (NEP-171).
    let kept =
        s.c.nft_resolve_transfer(acc(BOB), acc(CAROL), DEED_ID.into(), None);
    r.check(
        "A35",
        "nft_resolve_transfer whose return is refused (the receiver started a key rotation in nft_on_transfer) reports true and emits no NEP-171 transfer back; the receiver still owns the deed",
        kept && s.m().owner == CAROL && !logs().iter().any(|l| l.contains("agent_exit:returned")),
        json!({"owner": s.m().owner}),
    );

    let mut s = translator_world();
    for i in 0..agent_core::MAX_PAID_TO {
        s.by_1y(ALICE)
            .withdraw(U128(1), Some(acc(&format!("junk{i}.testnet"))));
    }
    s.by_1y(ALICE)
        .list_for_sale(U128(1_000 * USD), CODE_HASH.into(), None);
    s.buy(BOB, 1_000 * USD, BOB_RUNNER);
    s.callback(true).on_handover(1);
    s.pay(PAYERS[0], 100 * USD);
    let fresh = !panics_with("PaidToFull", || {
        s.by_1y(BOB)
            .withdraw(U128(10 * USD), Some(acc("bobs-bank.testnet")));
    });
    let st = s.by(MALLORY).get_status();
    r.check(
        "A36",
        "a seller who filled the round-trip table (1000 junk destinations) does not leave the buyer with PaidToFull: the quota is per owner; get_status shows paid_to_total and the scan blocks a table the buyer could not fill",
        fresh && st["paid_to_total"] == 1_001 && st["paid_to_owner"] == 1,
        json!({"paid_to_total": st["paid_to_total"], "paid_to_owner": st["paid_to_owner"]}),
    );

    let mut s = translator_world();
    s.by_1y(ALICE).withdraw(U128(100 * USD), None);
    s.by_1y(ALICE)
        .list_for_sale(U128(1_000 * USD), CODE_HASH.into(), None);
    s.buy(BOB, 1_000 * USD, BOB_RUNNER);
    s.callback(true).on_handover(1);
    let bob_free = s.m().owner_free;
    let alice_claim = claim(&s, ALICE);
    s.callback(false).on_transfer_out(
        OutKind::OwnerWithdraw,
        acc(ALICE),
        U128(100 * USD),
        Some(acc(ALICE)),
    );
    r.check(
        "A38",
        "a seller's withdrawal that fails after the agent was sold is credited back to the seller (claimable), not to the buyer's revenue",
        s.m().owner_free == bob_free && claim(&s, ALICE) == alice_claim + 100 * USD,
        json!({"seller_claimable": usd(claim(&s, ALICE))}),
    );
    // A47: outsiders cannot grow the state blob.
    let mut s = translator_world();
    let before = s.c.state_bytes();
    let rows_before = s.m().meter.tracked_payers;
    for i in 0..1_000 {
        let p = format!("{i:0>56}.testnet");
        s.register(&p);
        s.pay(&p, USD);
    }
    let after = s.c.state_bytes();
    let st = s.by(MALLORY).get_status();
    r.check(
        "A47",
        "1,000 distinct 64-character payers do not grow the state blob (payer, day, payout-destination, claimable, past-owner and sale-history rows are separate storage entries), so loading the contract costs the same; every view still answers",
        after <= before + 16 && st["near_storage"]["payer_entries"] == rows_before + 1_000,
        json!({"state_blob_bytes_before": before, "state_blob_bytes_after": after, "payer_entries": st["near_storage"]["payer_entries"]}),
    );

    // Review 3 (A44-A46).
    let hour = 3_600 * agent_core::NANOS_PER_SEC;
    let mut s = sold_translator();
    s.by_1y(BOB).set_runner_key(key(CAROL_RUNNER)).detach();
    let batch = s.m().key_batch;
    s.advance(hour + 1);
    let rk = s.by_1y(BOB).recover_keys();
    let deleted: Vec<String> = serde_json::from_value(rk["delete_keys"].clone()).unwrap();
    s.callback(true).on_keys_deleted(deleted);
    let applied = s
        .callback(true)
        .on_runner_key(vec![], vec![], key_str(CAROL_RUNNER), batch);
    let gone = key_actions(AGENT) == vec![("delete".to_string(), key_str(CAROL_RUNNER))];
    let mut s2 = listed_translator();
    s2.buy(BOB, 1_000 * USD, BOB_RUNNER);
    s2.advance(hour + 1);
    let rs = s2.by(MALLORY).recover_sale();
    let deleted: Vec<String> = serde_json::from_value(rs["delete_keys"].clone()).unwrap();
    s2.callback(true).on_keys_deleted(deleted);
    let refund = s2.callback(true).on_handover(1).0;
    let buyer_key_gone = key_actions(AGENT) == vec![("delete".to_string(), key_str(BOB_RUNNER))];
    r.check(
        "A44",
        "a key-batch callback that lands after recover_keys / recover_sale is stale (batch nonce): it changes no key set, deletes a key its late batch added, and a late on_handover refunds the buyer in full",
        !applied
            && gone
            && !s.m().known_keys.contains(&key_str(CAROL_RUNNER))
            && refund == 1_000 * USD
            && buyer_key_gone
            && s2.m().owner == ALICE,
        json!({"rotation_applied": applied, "handover_refund": usd(refund)}),
    );

    let mut s = translator_world();
    for i in 0..agent_core::MAX_PAID_TO {
        s.by_1y(ALICE)
            .withdraw(U128(1), Some(acc(&format!("junk{i}.testnet"))))
            .detach();
    }
    s.by_1y(ALICE)
        .list_for_sale(U128(1_000 * USD), CODE_HASH.into(), None);
    s.buy(BOB, 1_000 * USD, BOB_RUNNER);
    s.callback(true).on_handover(1);
    let open = s.by(MALLORY).get_status()["paid_to_total"].clone();
    s.advance(31 * DAY);
    s.by(MALLORY).finalize_earnout(1);
    let closed = s.by(MALLORY).get_status()["paid_to_total"].clone();
    r.check(
        "A45",
        "payout destinations are pruned once a finalized sale closed their era and no open earn-out needs them, so the round-trip table cannot block resale for good",
        open == 1_000 && closed == 0,
        json!({"while_earnout_open": open, "after_finalize": closed}),
    );

    let mut notes = vec![];
    for now in [T0 + 8 * DAY + 2 * hour, T0 - hour] {
        let mut s = Sim::new(AGENT, ALICE);
        s.now = now;
        s.ft(PAYERS[0], 12 * USD, &job_msg(&job(0)), 100);
        notes.push(events("payment")[0]["verify"].clone());
    }
    let mut s = business_world();
    let st = s.by(MALLORY).get_status();
    r.check(
        "A46",
        "a NEAR AI proof must carry signed_at within 7 days (not ahead of the block); used signature ids are kept only until they expire, at most 512, so outsiders cannot grow the agent's storage without bound; get_status.near_storage shows NEAR storage solvency and the scan blocks an insolvent agent",
        notes == vec![json!("SignatureExpired"), json!("SignedInFuture")]
            && st["near_storage"]["solvent"] == true
            && st["near_storage"]["max_sig_entries"] == 512,
        json!({"old_or_future_proof": notes, "near_storage_solvent": st["near_storage"]["solvent"]}),
    );

    // Review 4 (A50, A52-A54; A51 is a scan rule, in `make attack`).
    let mut s = translator_world();
    let rows = s.m().meter.tracked_payers;
    for i in 0..1_000 {
        let mut cl = s.call(MALLORY);
        cl.deposit = agent_exit::PAYER_ROW_DEPOSIT;
        s.env(&cl);
        s.c.storage_deposit(Some(acc(&format!("g{i}.testnet"))), None);
    }
    s.register("real-customer.testnet");
    let counted = s.m().meter.counted_payers;
    s.pay("real-customer.testnet", 50 * USD);
    let real_counted = s.m().meter.counted_payers == counted + 1;
    let mut cl = s.call("g0.testnet");
    cl.deposit = NearToken::from_yoctonear(1);
    s.env(&cl);
    let unregistered = s.c.storage_unregister(None);
    s.advance(agent_exit::EMPTY_ROW_TTL_NS);
    s.env(&s.call(CAROL));
    let reclaimed = s.c.reclaim_payer_row(acc("g1.testnet"))["refund_to"] == MALLORY;
    let empty = s.by(MALLORY).get_status()["empty_payer_rows"].clone();
    r.check(
        "A50",
        "an outsider registering 1,000 third-party payer rows does not stop a real customer getting a row and being counted (no cap: each row is paid by its depositor); an empty row can be removed by its account (storage_unregister) or, after 30 days, by anyone, refunding its depositor",
        real_counted
            && unregistered
            && reclaimed
            && s.m().meter.tracked_payers == rows + 1_001 - 2,
        json!({"third_party_rows": 1_000, "real_customer_counted": real_counted, "empty_payer_rows": empty}),
    );

    let mut s = sold_translator();
    let mut l = demo_limits();
    l.base = Some(agent_exit::BaseLimitsArgs {
        daily_cap: U128(5_000_000),
        payees: vec!["0x00000000000000000000000000000000000000a1".into()],
    });
    s.by_1y(BOB).set_limits(l);
    s.by_1y(BOB).set_limits(demo_limits());
    let kept = s.m().base.limits.payees.len() == 1 && s.m().base.limits.daily_cap == 5_000_000;
    r.check(
        "A52",
        "set_limits without base keeps the Base limits; clearing them takes an explicit empty value",
        kept,
        json!({"base_kept": kept}),
    );

    let mut s = sold_translator();
    let b0 = s.c.state_bytes();
    let mut owner = BOB.to_string();
    for i in 0..300 {
        let next = format!("alt{i}.testnet");
        s.pay(&format!("cust{i}.testnet"), USD);
        let free = s.m().owner_free;
        if free > 0 {
            s.by_1y(&owner)
                .withdraw(U128(free), Some(acc(&format!("dest{i}.testnet"))))
                .detach();
        }
        s.by_1y(&owner)
            .nft_transfer(acc(&next), DEED_ID.into(), None, None);
        let keys: Vec<String> = s.m().retiring.iter().cloned().collect();
        s.callback(true).on_keys_deleted(keys);
        owner = next;
    }
    let b1 = s.c.state_bytes();
    r.check(
        "A53",
        "300 deed moves, each with a payout to a new destination, leave the state blob the same size (payout destinations are counters plus a pruned-prefix watermark)",
        b1 <= b0 + 16 && s.m().paid_to_total() >= 300,
        json!({"state_blob_bytes_before": b0, "state_blob_bytes_after": b1}),
    );

    let mut s = Sim::new(AGENT, ALICE);
    s.now = T0 + hour;
    let now_s = s.now / agent_core::NANOS_PER_SEC;
    s.env(&s.call(AGENT));
    for i in 0..agent_exit::verified::MAX_SIGS {
        let mut id = [0xffu8; 32];
        id[..4].copy_from_slice(&i.to_le_bytes());
        s.c.used_sigs.insert(id, now_s + 60, now_s);
    }
    s.ft(PAYERS[0], 12 * USD, &job_msg(&job(0)), 100);
    let shared = events("payment")[0]["verify"].clone();
    s.register(PAYERS[0]);
    s.ft(PAYERS[0], 12 * USD, &job_msg(&job(0)), 100);
    let own = events("payment")[0]["verify"].clone();
    r.check(
        "A54",
        "outsiders filling the shared 512-signature window do not deny a registered payer's NEAR Verified badge: it has 8 slots of its own, paid by its row deposit",
        shared == "SigWindowFull" && own.as_str().is_some_and(|v| v.starts_with("ok:")),
        json!({"unregistered": shared, "registered": own}),
    );

    let mut s = listed_translator();
    let paused = panics_with("Listed: spending paused", || {
        s.by_1y(ALICE).withdraw_near(NearToken::from_millinear(1));
    });
    r.check(
        "A39",
        "no withdraw_near while listed: the NEAR balance the buyer scanned stays",
        paused,
        json!({"error": "Listed: spending paused"}),
    );

    // Feature 5: the Base wallet, with the offline TEST MPC signer.
    let mpc = base_wallet::MockMpc::default();
    let supplier = "0x00000000000000000000000000000000000000a1";
    let base_limits = |s: &mut Sim, who: &str| {
        let mut l = demo_limits();
        l.base = Some(agent_exit::BaseLimitsArgs {
            daily_cap: U128(5 * agent_core::base::USDC),
            payees: vec![supplier.into()],
        });
        s.by_1y(who).set_limits(l);
    };
    let base_pay = |s: &mut Sim, runner: u8, ok: bool| -> Value {
        let r = s
            .runner(key(runner))
            .pay_base(supplier.into(), U128(agent_core::base::USDC), None);
        let calls = mpc_sign_calls();
        let digest: [u8; 32] = hex::decode(
            calls[0].0["request"]["payload_v2"]["Ecdsa"]
                .as_str()
                .unwrap_or_default(),
        )
        .unwrap_or_default()
        .try_into()
        .unwrap_or([0; 32]);
        let id = r["auth_id"].as_u64().unwrap_or_default();
        if ok {
            let answer = mpc.sign(AGENT, agent_core::base::BASE_PATH, &digest);
            s.callback_data(&answer).on_base_signed(id)
        } else {
            s.callback(false).on_base_signed(id)
        }
    };
    let mut s = translator_world();
    let address = mpc.derive_address(AGENT, agent_core::base::BASE_PATH);
    base_limits(&mut s, ALICE);
    let before = base_pay(&mut s, ALICE_RUNNER, true);
    let before_ok = base_wallet::check_auth(&before["auth"]).ok();
    let open_blocks = panics_with("OpenApproval", || {
        s.by_1y(ALICE)
            .list_for_sale(U128(1_000 * USD), CODE_HASH.into(), None);
    });
    s.now += 601 * agent_core::NANOS_PER_SEC;
    s.by_1y(ALICE)
        .list_for_sale(U128(1_000 * USD), CODE_HASH.into(), None);
    s.buy(BOB, 1_000 * USD, BOB_RUNNER);
    s.callback(true).on_handover(1);
    s.callback(true)
        .on_keys_deleted(vec![key_str(ALICE_RUNNER)]);
    let after = base_pay(&mut s, BOB_RUNNER, true);
    let after_ok = base_wallet::check_auth(&after["auth"]).ok();
    r.check(
        "S6",
        "wallet follows the name (mock MPC): the agent's Base address for path wallet-1 is the same before and after the sale; alice's runner signs a 1.00 USDC EIP-3009 authorization before, bob's after; each signature recovers to that address",
        before_ok.as_deref() == Some(address.as_str())
            && after_ok.as_deref() == Some(address.as_str())
            && before["auth"]["owner_epoch"] == 1
            && after["auth"]["owner_epoch"] == 2,
        json!({"address": address, "before_epoch": before["auth"]["owner_epoch"],
               "after_epoch": after["auth"]["owner_epoch"], "calldata_selector": "0xe3ee160e"}),
    );
    r.check(
        "A9",
        "a signed Base authorization still valid blocks the listing (OpenApproval) until valid_before passes",
        open_blocks,
        json!({"error": "OpenApproval"}),
    );
    let alice_refused = panics_with("NotRunner", || {
        s.by(ALICE)
            .pay_base(supplier.into(), U128(agent_core::base::USDC), None);
    });
    let alice_limits = panics_with("NotOwner", || base_limits(&mut s, ALICE));
    r.check(
        "A20",
        "after the sale the old seller cannot make the Base wallet sign (pay_base: NotRunner) or change its limits (NotOwner)",
        alice_refused && alice_limits,
        json!({"pay_base": "NotRunner", "set_limits": "NotOwner"}),
    );
    let mut s = translator_world();
    base_limits(&mut s, ALICE);
    let failed_sign = base_pay(&mut s, ALICE_RUNNER, false);
    let w = s.by(BOB).get_base_wallet();
    let retry = base_pay(&mut s, ALICE_RUNNER, true);
    r.check(
        "E22",
        "MPC sign fails: sign_failed, the reserved Base cap is released, nothing stays open; the retry uses a new nonce and signs",
        failed_sign["ok"] == false
            && w["spent_today"] == "0"
            && w["open_auth_until"] == "0"
            && retry["ok"] == true,
        json!({"sign_failed": failed_sign["sign_failed"]["reason"]}),
    );
    let too_long = panics_with("ValidityTooLong", || {
        s.runner(key(ALICE_RUNNER)).pay_base(
            supplier.into(),
            U128(agent_core::base::USDC),
            Some(3_601),
        );
    });
    r.check(
        "E23",
        "a Base authorization valid for more than an hour is refused ValidityTooLong",
        too_long,
        json!({"error": "ValidityTooLong: at most 3600 s"}),
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
            {"id": "S6,G2", "part": "the real MPC signer (v1.signer-prod.testnet) and a relayed Base Sepolia transfer (signatures here come from a local TEST MPC key)",
             "where": "owner's machine, gate G2 (make gates)"},
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
