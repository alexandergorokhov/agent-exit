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
        root: None,
    };
    assert_eq!(m.commit_config("alice", Some(bad)), Err(Error::BadConfig));
    let big = ConfigCommit {
        hash: "a".repeat(64),
        bytes: sealed::MAX_CONFIG_BYTES + 1,
        root: None,
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

/// SC17 (review 4 H1): a backdoor proven after a sealed sale, before the
/// delivery clears, returns the held upfront part to the buyer too.
#[test]
fn sc17_backdoor_after_sealed_sale_refunds_upfront_too() {
    let mut m = sold_sealed();
    assert_eq!(m.held, 1_000 * USD);
    let r = m.runner_guard("translator", "ed25519:hidden").unwrap();
    assert!(matches!(r, RunnerCheck::BackdoorProven { .. }));
    assert_eq!(m.claimable_of("bob"), 1_000 * USD, "the whole price back");
    assert_eq!(m.claimable_of("alice"), 0);
    assert_eq!(m.upfront_held_total(), 0);
    assert_eq!(
        m.held,
        m.escrow_left()
            + m.upfront_held_total()
            + m.claimable_total()
            + m.owner_free
            + m.treasury
            + m.share_escrow,
        "every token in a bucket"
    );
    assert!(m.solvent());
}

/// SC18 (review 4 H2): a low-order ephemeral key is refused at delivery, so
/// a seller cannot post a bundle no client can open or dispute.
#[test]
fn sc18_low_order_ephemeral_key_refused() {
    let mut m = sold_sealed();
    let cipher = vec![0u8; BUNDLE.len()];
    for bad in ["00".repeat(32), format!("01{}", "00".repeat(31))] {
        assert_eq!(
            m.deliver_config("alice", 1, &bad, &cipher, T0 + DAY_NS),
            Err(Error::BadConfig)
        );
    }
    assert!(!config(&m).delivered());
}

/// SC19 (review 4 M1): the held upfront part is in the exact ledger and in
/// solvency while a sealed sale waits.
#[test]
fn sc19_upfront_held_is_in_the_ledger() {
    let m = sold_sealed();
    assert_eq!(m.upfront_held_total(), 700 * USD);
    assert_eq!(
        m.held,
        m.escrow_left()
            + m.upfront_held_total()
            + m.claimable_total()
            + m.owner_free
            + m.treasury
            + m.share_escrow
    );
}

/// SC20 (review 4 L2): a commitment does not pass with the deed.
#[test]
fn sc20_commitment_does_not_pass_with_the_deed() {
    let mut m = Machine::new("translator", "alice");
    let deleted = m.set_runner_key("alice", ALICE_PK, T0).unwrap();
    m.runner_key_done(m.key_batch, &[], &deleted, ALICE_PK, true);
    m.commit_config("alice", Some(ConfigCommit::of(BUNDLE)))
        .unwrap();
    m.transfer_deed("alice", "bob", None, T0).unwrap();
    assert_eq!(m.config_commit, None, "bob commits his own bundle");
}

// ------------------------------------------------ SC21-SC24: chunk preview

/// SC21: every chunk of bundles of awkward sizes proves against the root;
/// a changed chunk, a changed proof or the wrong index does not.
#[test]
fn sc21_chunk_proofs() {
    for len in [1usize, 255, 256, 257, 700, 1_024, 2_049, 8_192] {
        let b: Vec<u8> = (0..len).map(|i| (i * 7 + len) as u8).collect();
        let c = ConfigCommit::of(&b);
        assert_eq!(c.chunks(), (len as u32).div_ceil(sealed::CHUNK));
        for i in 0..c.chunks() {
            let (chunk, proof) = sealed::chunk_proof(&b, i).unwrap();
            assert!(
                sealed::verify_chunk(&c, i, &chunk, &proof),
                "len {len} chunk {i}"
            );
            let mut bad = chunk.clone();
            bad[0] ^= 1;
            assert!(!sealed::verify_chunk(&c, i, &bad, &proof));
            if c.chunks() > 1 {
                assert!(!sealed::verify_chunk(
                    &c,
                    (i + 1) % c.chunks(),
                    &chunk,
                    &proof
                ));
                let mut p2 = proof.clone();
                p2[0] = "00".repeat(32);
                assert!(!sealed::verify_chunk(&c, i, &chunk, &p2));
                let mut p3 = proof.clone();
                p3.push("00".repeat(32));
                assert!(
                    !sealed::verify_chunk(&c, i, &chunk, &p3),
                    "an extra proof step"
                );
            }
        }
        assert!(!sealed::verify_chunk(&c, c.chunks(), &[], &[]));
        assert!(!sealed::verify_chunk(
            &ConfigCommit::without_root(&b),
            0,
            &b[..b.len().min(256)],
            &[]
        ));
    }
}

fn listed_with(commit: ConfigCommit) -> Machine {
    let mut m = Machine::new("translator", "alice");
    let deleted = m.set_runner_key("alice", ALICE_PK, T0).unwrap();
    m.runner_key_done(m.key_batch, &[], &deleted, ALICE_PK, true);
    m.commit_config("alice", Some(commit)).unwrap();
    m.list("alice", 1_000 * USD, Terms::default(), "hash", T0)
        .unwrap();
    m
}

/// SC22: bob asks for random chunks; the seller cannot ask, cannot pick,
/// and must prove each one against the committed root.
#[test]
fn sc22_preview_random_chunks_proven_by_the_seller() {
    let big: Vec<u8> = BUNDLE.iter().cycle().take(2_048).cloned().collect();
    let mut m = listed_with(ConfigCommit::of(&big));
    assert_eq!(
        m.request_preview("alice", &[7; 32], T0),
        Err(Error::NotBuyer)
    );
    let idx = m.request_preview("bob", &[7; 32], T0).unwrap();
    assert_eq!(idx.len(), sealed::PREVIEW_CHUNKS as usize);
    assert!(idx[0] < idx[1] && idx[1] < 8, "distinct, in range: {idx:?}");
    assert_eq!(
        idx,
        sealed::preview_indices(&[7; 32], 2_048),
        "fixed by the seed"
    );
    assert_eq!(
        m.request_preview("bob", &[9; 32], T0),
        Err(Error::PreviewTaken),
        "one draw per buyer per listing version"
    );
    // Review 5 P2: a stranger's draw does not block bob's, and is not his.
    assert!(m.request_preview("carol", &[9; 32], T0).is_ok());
    assert!(m.current_preview("carol").is_some());
    let other = (0..8).find(|i| !idx.contains(i)).unwrap();
    let (c, p) = sealed::chunk_proof(&big, other).unwrap();
    assert_eq!(
        m.answer_preview("alice", "bob", other, &c, &p),
        Err(Error::NotRequested)
    );
    let (c0, p0) = sealed::chunk_proof(&big, idx[0]).unwrap();
    assert_eq!(
        m.answer_preview("bob", "bob", idx[0], &c0, &p0),
        Err(Error::NotOwner)
    );
    let mut fake = c0.clone();
    fake[3] ^= 0x20;
    assert_eq!(
        m.answer_preview("alice", "bob", idx[0], &fake, &p0),
        Err(Error::BadChunk)
    );
    assert_eq!(m.answer_preview("alice", "bob", idx[0], &c0, &p0), Ok(false));
    assert_eq!(
        m.answer_preview("alice", "bob", idx[0], &c0, &p0),
        Err(Error::NotRequested)
    );
    let (c1, p1) = sealed::chunk_proof(&big, idx[1]).unwrap();
    assert_eq!(m.answer_preview("alice", "bob", idx[1], &c1, &p1), Ok(true));
    let pv = m.current_preview("bob").unwrap();
    assert!(pv.complete());
    assert_eq!(pv.proven[0], (idx[0], c0));
    // A relist is a new version: a new request draws new chunks.
    m.cancel_listing("alice").unwrap();
    m.list("alice", 900 * USD, Terms::default(), "hash", T0)
        .unwrap();
    assert!(m.current_preview("bob").is_none());
    assert_eq!(m.preview_draws, 2, "draws are never forgotten");
    assert!(m.request_preview("bob", &[8; 32], T0).is_ok());
}

/// SC23: a seller who commits a hash of one bundle and a root of another
/// loses the sale whichever one she delivers.
#[test]
fn sc23_hash_and_root_must_name_the_same_bundle() {
    let shown: Vec<u8> = vec![b'A'; BUNDLE.len()];
    for deliver_shown in [false, true] {
        let commit = ConfigCommit {
            root: ConfigCommit::of(&shown).root,
            ..ConfigCommit::of(BUNDLE)
        };
        let mut m = listed_with(commit);
        let h = m.known_keys_hash();
        let v = m.listing_version;
        let plan = m
            .begin_buy_sealed(
                "bob",
                1_000 * USD,
                v,
                &h,
                BOB_PK,
                Some(&bob_delivery_pk()),
                true,
                T0,
            )
            .unwrap();
        m.finish_buy(plan.sale_id, true, T0).unwrap();
        let what: &[u8] = if deliver_shown { &shown } else { BUNDLE };
        let cipher = deliver(&mut m, what, T0 + DAY_NS);
        let r = m.dispute_config(
            "bob",
            1,
            &hex::encode(BOB_DELIVERY_SK),
            &cipher,
            T0 + 2 * DAY_NS,
        );
        assert_eq!(r, Ok(1_000 * USD), "deliver_shown {deliver_shown}");
    }
}

/// SC24: a commitment without a root sells as before, with no preview.
#[test]
fn sc24_no_root_no_preview() {
    let mut m = listed_with(ConfigCommit::without_root(BUNDLE));
    assert_eq!(
        m.request_preview("bob", &[1; 32], T0),
        Err(Error::NoPreview)
    );
}

/// SC21b: the Node tool (`scripts/sealed-config.mjs proof`) builds the same
/// tree: a 700-byte vector, its root and the path of chunk 2.
#[test]
fn sc21b_chunk_tree_matches_the_node_tool() {
    let b: Vec<u8> = (0..700usize).map(|i| ((i * 7 + 700) % 256) as u8).collect();
    assert_eq!(
        hex::encode(sealed::chunk_root(&b)),
        "a98742ba87c21191996689eaac90d759a3f5ceca67a802a36bb9254a4e170ff4"
    );
    let (_, proof) = sealed::chunk_proof(&b, 2).unwrap();
    assert_eq!(
        proof,
        vec!["e24e296c32e9a9d8e849b85d7532deafae3c0d936a1b116a620b254dcd56f99f".to_string()]
    );
}

/// SC25: a preview never shows more than a quarter of the bundle, so a small
/// config (under 4 chunks) cannot be previewed at all.
#[test]
fn sc25_preview_never_gives_the_config_away() {
    // Counted in bytes (review 5 P3): 769 bytes would be 33% in one chunk.
    assert_eq!(sealed::preview_size(256), 0);
    assert_eq!(sealed::preview_size(769), 0);
    assert_eq!(sealed::preview_size(1_023), 0);
    assert_eq!(sealed::preview_size(1_024), 1);
    assert_eq!(sealed::preview_size(2_047), 1);
    assert_eq!(sealed::preview_size(2_048), sealed::PREVIEW_CHUNKS);
    assert_eq!(sealed::preview_size(8_192), sealed::PREVIEW_CHUNKS);
    let small: Vec<u8> = BUNDLE.iter().cycle().take(1_000).cloned().collect();
    let mut m = listed_with(ConfigCommit::of(&small));
    assert_eq!(m.request_preview("bob", &[7; 32], T0), Err(Error::NoPreview));
    let one: Vec<u8> = BUNDLE.iter().cycle().take(1_100).cloned().collect();
    let mut m = listed_with(ConfigCommit::of(&one));
    assert_eq!(m.request_preview("bob", &[7; 32], T0).unwrap().len(), 1);
}

/// SC26 (review 5 P2): previews are per buyer and capped; the seller cannot
/// answer while not listed; every draw counts.
#[test]
fn sc26_previews_are_per_buyer_and_capped() {
    let big: Vec<u8> = BUNDLE.iter().cycle().take(4_096).cloned().collect();
    let mut m = listed_with(ConfigCommit::of(&big));
    for (n, who) in ["b1", "b2", "b3", "b4"].iter().enumerate() {
        m.request_preview(who, &[n as u8; 32], T0).unwrap();
    }
    assert_eq!(
        m.request_preview("b5", &[9; 32], T0),
        Err(Error::PreviewsFull)
    );
    assert_eq!(m.current_previews().len(), sealed::MAX_PREVIEWS);
    let idx = m.current_preview("b2").unwrap().indices.clone();
    let (c, p) = sealed::chunk_proof(&big, idx[0]).unwrap();
    assert_eq!(
        m.answer_preview("alice", "nobody", idx[0], &c, &p),
        Err(Error::NotRequested)
    );
    // A relist is a new version: old draws are dropped, the count stays.
    m.cancel_listing("alice").unwrap();
    assert_eq!(
        m.answer_preview("alice", "b2", idx[0], &c, &p),
        Err(Error::NoPreview),
        "not listed"
    );
    m.list("alice", 900 * USD, Terms::default(), "hash", T0)
        .unwrap();
    assert!(m.request_preview("b5", &[9; 32], T0).is_ok());
    assert_eq!(m.current_previews().len(), 1);
    assert_eq!(m.preview_draws, 5);
}
