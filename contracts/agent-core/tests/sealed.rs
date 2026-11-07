//! SC1-SC10: the sealed agent config, a fair exchange of the off-chain part
//! of an agent (system prompt, tool setup, the API keys to rotate).

use agent_core::sealed::{self, ConfigCommit, Delivery};
use agent_core::*;

const T0: Ts = 1_767_225_600 * NANOS_PER_SEC;
const ALICE_PK: &str = "ed25519:alice-runner";
const BOB_PK: &str = "ed25519:bob-runner";
const BUNDLE: &[u8] = br#"{"schema":"agent-config.v1","system_prompt":"You translate EN<->ES. Be exact.","model":"deepseek-v3","tools":["glossary"],"rotate_after_sale":["NEARAI_API_KEY"]}"#;
const BOB_DELIVERY_SK: [u8; 32] = [42u8; 32];
const SELLER_EPH_SK: [u8; 32] = [17u8; 32];

fn bob_delivery_pk() -> String {
    hex::encode(sealed::public_key(&BOB_DELIVERY_SK))
}

/// alice commits to BUNDLE, lists at 1,000 (default terms), bob buys with a
/// one-time delivery key.
fn sold_sealed() -> Machine {
    let mut m = Machine::new("translator", "alice");
    let deleted = m.set_runner_key("alice", ALICE_PK, T0).unwrap();
    m.runner_key_done(m.key_batch, &[], &deleted, ALICE_PK, true);
    m.commit_config("alice", Some(ConfigCommit::of(BUNDLE)))
        .unwrap();
    let l = m
        .list("alice", 1_000 * USD, Terms::default(), "hash", T0)
        .unwrap();
    assert_eq!(
        l.config,
        Some(ConfigCommit::of(BUNDLE)),
        "SC1: the listing shows the commitment"
    );
    let h = m.known_keys_hash();
    let pk = bob_delivery_pk();
    assert_eq!(
        m.begin_buy("bob", 1_000 * USD, l.version, &h, BOB_PK, true, T0),
        Err(Error::DeliveryKeyRequired),
        "SC2"
    );
    let plan = m
        .begin_buy_sealed(
            "bob",
            1_000 * USD,
            l.version,
            &h,
            BOB_PK,
            Some(&pk),
            true,
            T0,
        )
        .unwrap();
    m.finish_buy(plan.sale_id, true, T0).unwrap();
    m
}

fn deliver(m: &mut Machine, bundle: &[u8], at: Ts) -> Vec<u8> {
    let (eph, cipher) = sealed::seal(
        &SELLER_EPH_SK,
        &sealed::hex32(&bob_delivery_pk()).unwrap(),
        bundle,
    )
    .unwrap();
    m.deliver_config("alice", 1, &hex::encode(eph), &cipher, at)
        .unwrap();
    cipher
}

fn config(m: &Machine) -> sealed::SaleConfig {
    m.sale(1).unwrap().config.clone().unwrap()
}

#[test]
fn sc3_honest_delivery_opens_to_the_committed_bundle() {
    let mut m = sold_sealed();
    let cipher = deliver(&mut m, BUNDLE, T0 + DAY_NS);
    let Delivery::Delivered { ephemeral_pk, .. } = config(&m).state else {
        panic!()
    };
    let plain = sealed::open(
        &BOB_DELIVERY_SK,
        &sealed::hex32(&ephemeral_pk).unwrap(),
        &cipher,
    )
    .unwrap();
    assert_eq!(plain, BUNDLE);
    assert_eq!(
        m.deliver_config("alice", 1, &ephemeral_pk, &cipher, T0 + DAY_NS),
        Err(Error::AlreadyDelivered)
    );
    assert!(m.solvent());
}

#[test]
fn sc4_no_release_before_delivery() {
    let mut m = sold_sealed();
    m.pay("c1", 100 * USD, false, T0 + 3_600 * NANOS_PER_SEC);
    assert_eq!(
        m.sale(1).unwrap().released,
        0,
        "held until the bundle arrives"
    );
    deliver(&mut m, BUNDLE, T0 + DAY_NS);
    m.pay("c2", 100 * USD, false, T0 + DAY_NS + 1);
    assert_eq!(m.sale(1).unwrap().released, 0, "SC14: held until accepted");
    assert_eq!(m.claimable_of("alice"), 0, "SC14: nothing paid up front");
    assert_eq!(m.release_upfront("bob", 1, T0 + DAY_NS + 2), Ok(700 * USD));
    assert_eq!(m.claimable_of("alice"), 700 * USD);
    m.pay("c3", 100 * USD, false, T0 + DAY_NS + 3);
    assert_eq!(m.sale(1).unwrap().released, 50 * USD);
    assert!(m.solvent());
}

#[test]
fn sc5_no_delivery_by_the_deadline_returns_the_escrow_to_the_buyer() {
    let mut m = sold_sealed();
    let by = config(&m).deliver_by;
    assert_eq!(m.claim_undelivered(1, by), Err(Error::DeliveryOpen));
    assert_eq!(
        m.claim_undelivered(1, by + 1),
        Ok(1_000 * USD),
        "SC14: the whole price, upfront part included"
    );
    assert_eq!(m.claimable_of("bob"), 1_000 * USD);
    assert_eq!(m.claimable_of("alice"), 0);
    let (eph, c) = sealed::seal(
        &SELLER_EPH_SK,
        &sealed::hex32(&bob_delivery_pk()).unwrap(),
        BUNDLE,
    )
    .unwrap();
    assert_eq!(
        m.deliver_config("alice", 1, &hex::encode(eph), &c, by + 2),
        Err(Error::UnknownSale)
    );
    assert!(m.solvent());
}

#[test]
fn sc6_undelivered_at_window_end_earns_no_floor() {
    let mut m = sold_sealed();
    let end = m.sale(1).unwrap().window_end;
    let s = m.finalize_earnout(1, end).unwrap();
    assert_eq!(s.returned, 300 * USD);
    assert_eq!(s.released, 0, "no floor for an undelivered bundle");
    assert_eq!(m.claimable_of("alice"), 0);
    assert_eq!(m.claimable_of("bob"), 1_000 * USD);
}

#[test]
fn sc8_a_wrong_bundle_is_proven_on_chain_and_refunded() {
    let mut m = sold_sealed();
    // Same length, different prompt: the seller swapped the brain.
    let mut fake = BUNDLE.to_vec();
    fake[60] = b'X';
    let cipher = deliver(&mut m, &fake, T0 + DAY_NS);
    let sk = hex::encode(BOB_DELIVERY_SK);
    assert_eq!(
        m.dispute_config("alice", 1, &sk, &cipher, T0 + 2 * DAY_NS),
        Err(Error::NotBuyer)
    );
    assert_eq!(
        m.dispute_config("bob", 1, &hex::encode([1u8; 32]), &cipher, T0 + 2 * DAY_NS),
        Err(Error::BadDeliveryKey)
    );
    assert_eq!(
        m.dispute_config("bob", 1, &sk, &cipher, T0 + 2 * DAY_NS),
        Ok(1_000 * USD)
    );
    assert_eq!(m.claimable_of("bob"), 1_000 * USD, "SC14: the whole price");
    assert_eq!(m.claimable_of("alice"), 0);
    let s = m.sale(1).unwrap();
    assert!(s.cut && s.finalized);
    assert!(matches!(
        s.config.as_ref().unwrap().state,
        Delivery::Mismatch { .. }
    ));
    assert!(m.solvent());
}

#[test]
fn sc9_a_frivolous_dispute_is_refused() {
    let mut m = sold_sealed();
    let cipher = deliver(&mut m, BUNDLE, T0 + DAY_NS);
    let sk = hex::encode(BOB_DELIVERY_SK);
    assert_eq!(
        m.dispute_config("bob", 1, &sk, &cipher, T0 + 2 * DAY_NS),
        Err(Error::ConfigMatches)
    );
    // And after the window it is closed.
    let until = config(&m).dispute_until();
    assert_eq!(
        m.dispute_config("bob", 1, &sk, &cipher, until + 1),
        Err(Error::DisputeClosed)
    );
    // Finalize waits for the dispute window, then pays as usual.
    m.pay("c1", 100 * USD, false, T0 + 3 * DAY_NS);
    let s = m
        .finalize_earnout(1, m.sale(1).unwrap().window_end)
        .unwrap();
    assert_eq!(
        s.released,
        60 * USD,
        "SC14: no release inside the dispute window; the floor is 20% of 300"
    );
    assert_eq!(m.claimable_of("alice"), 760 * USD, "700 upfront + 60");
    assert!(m.solvent());
}

#[test]
fn sc14_the_upfront_part_waits_for_acceptance() {
    let mut m = sold_sealed();
    assert_eq!(
        m.release_upfront("bob", 1, T0 + 1),
        Err(Error::DeliveryOpen),
        "nothing to accept yet"
    );
    let mut fake = BUNDLE.to_vec();
    fake[60] = b'X';
    let cipher = deliver(&mut m, &fake, T0 + DAY_NS);
    // Customers pay during the dispute window: nothing goes to alice.
    m.pay("c1", 100 * USD, false, T0 + 2 * DAY_NS);
    m.pay("c2", 100 * USD, false, T0 + 2 * DAY_NS);
    assert_eq!(
        m.release_upfront("alice", 1, T0 + 2 * DAY_NS),
        Err(Error::DisputeOpen),
        "the seller cannot clear it early"
    );
    assert_eq!(m.claimable_of("alice"), 0);
    let sk = hex::encode(BOB_DELIVERY_SK);
    assert_eq!(
        m.dispute_config("bob", 1, &sk, &cipher, T0 + 3 * DAY_NS),
        Ok(1_000 * USD),
        "the judge-18 attack: alice keeps nothing"
    );
    assert_eq!(m.claimable_of("alice"), 0);
    assert!(m.solvent());
}

#[test]
fn sc14b_accepted_or_undisputed_pays_the_seller_and_closes_disputes() {
    let mut m = sold_sealed();
    let cipher = deliver(&mut m, BUNDLE, T0 + DAY_NS);
    let until = config(&m).dispute_until();
    assert_eq!(
        m.release_upfront("carol", 1, until),
        Err(Error::DisputeOpen)
    );
    assert_eq!(m.release_upfront("carol", 1, until + 1), Ok(700 * USD));
    assert_eq!(
        m.release_upfront("carol", 1, until + 2),
        Ok(0),
        "idempotent"
    );
    assert_eq!(m.claimable_of("alice"), 700 * USD);
    // Accepted by the buyer early: the dispute is closed too.
    let mut m = sold_sealed();
    let cipher2 = deliver(&mut m, BUNDLE, T0 + DAY_NS);
    assert_eq!(cipher, cipher2);
    m.release_upfront("bob", 1, T0 + DAY_NS + 1).unwrap();
    assert_eq!(
        m.dispute_config(
            "bob",
            1,
            &hex::encode(BOB_DELIVERY_SK),
            &cipher2,
            T0 + 2 * DAY_NS
        ),
        Err(Error::DisputeClosed)
    );
    assert!(m.solvent());
}

#[test]
fn sc10_commitment_rules() {
    let mut m = Machine::new("translator", "alice");
    assert_eq!(
        m.commit_config("bob", Some(ConfigCommit::of(BUNDLE))),
        Err(Error::NotOwner)
    );
    let bad = ConfigCommit {
        hash: "ab".into(),
        bytes: 3,
    };
    assert_eq!(m.commit_config("alice", Some(bad)), Err(Error::BadConfig));
    let big = ConfigCommit {
        hash: "a".repeat(64),
        bytes: sealed::MAX_CONFIG_BYTES + 1,
    };
    assert_eq!(m.commit_config("alice", Some(big)), Err(Error::BadConfig));
    m.commit_config("alice", Some(ConfigCommit::of(BUNDLE)))
        .unwrap();
    m.list("alice", 1_000 * USD, Terms::default(), "h", T0)
        .unwrap();
    assert_eq!(
        m.commit_config("alice", None),
        Err(Error::AlreadyListed),
        "the buyer scanned it"
    );
    // A wrong-length delivery is refused, a late one too.
    let mut m = sold_sealed();
    let eph = hex::encode(sealed::public_key(&SELLER_EPH_SK));
    assert_eq!(
        m.deliver_config("alice", 1, &eph, b"short", T0),
        Err(Error::BadConfig)
    );
    assert_eq!(
        m.deliver_config("bob", 1, &eph, BUNDLE, T0),
        Err(Error::NotSeller)
    );
    let by = config(&m).deliver_by;
    assert_eq!(
        m.deliver_config("alice", 1, &eph, BUNDLE, by + 1),
        Err(Error::DeliveryLate)
    );
    // The commitment is cleared by the sale.
    assert_eq!(m.config_commit, None);
}

#[test]
fn sc11_listings_without_a_config_are_unchanged() {
    let mut m = Machine::new("translator", "alice");
    let deleted = m.set_runner_key("alice", ALICE_PK, T0).unwrap();
    m.runner_key_done(m.key_batch, &[], &deleted, ALICE_PK, true);
    let l = m
        .list("alice", 1_000 * USD, Terms::default(), "h", T0)
        .unwrap();
    assert_eq!(l.config, None);
    let h = m.known_keys_hash();
    let plan = m
        .begin_buy_sealed(
            "bob",
            1_000 * USD,
            l.version,
            &h,
            BOB_PK,
            Some(&bob_delivery_pk()),
            true,
            T0,
        )
        .unwrap();
    m.finish_buy(plan.sale_id, true, T0).unwrap();
    assert_eq!(m.sale(1).unwrap().config, None);
    m.pay("c1", 100 * USD, false, T0 + 1);
    assert_eq!(m.sale(1).unwrap().released, 50 * USD);
}

#[test]
fn sc15_a_low_order_delivery_key_is_refused_at_the_buy() {
    let mut m = Machine::new("translator", "alice");
    let deleted = m.set_runner_key("alice", ALICE_PK, T0).unwrap();
    m.runner_key_done(m.key_batch, &[], &deleted, ALICE_PK, true);
    m.commit_config("alice", Some(ConfigCommit::of(BUNDLE)))
        .unwrap();
    let l = m
        .list("alice", 1_000 * USD, Terms::default(), "hash", T0)
        .unwrap();
    let h = m.known_keys_hash();
    // u = 1 and u = 0 are low-order: nothing can be sealed to them, so a
    // buyer could otherwise force "undelivered" and take the price back.
    let mut one = [0u8; 32];
    one[0] = 1;
    for bad in [hex::encode(one), hex::encode([0u8; 32])] {
        assert_eq!(
            m.begin_buy_sealed(
                "bob",
                1_000 * USD,
                l.version,
                &h,
                BOB_PK,
                Some(&bad),
                true,
                T0
            ),
            Err(Error::BadDeliveryKey),
            "{bad}"
        );
    }
    assert!(sealed::usable_key(&sealed::public_key(&BOB_DELIVERY_SK)));
}

#[test]
fn sc16_finalize_waits_for_the_delivery_deadline_and_reports_the_whole_refund() {
    let mut m = sold_sealed();
    let by = config(&m).deliver_by;
    // A short earn-out window (demo builds) must not cut the delivery window.
    m.sales[0].window_end = T0 + 600 * NANOS_PER_SEC;
    assert_eq!(
        m.finalize_earnout(1, T0 + 660 * NANOS_PER_SEC),
        Err(Error::DeliveryOpen)
    );
    let s = m.finalize_earnout(1, by + 1).unwrap();
    assert_eq!(s.returned, 300 * USD, "the escrow part");
    assert_eq!(s.back_to_buyer(), 1_000 * USD, "the whole price");
}
