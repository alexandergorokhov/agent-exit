//! K8: the contract refuses to sell an agent that was not born clean.
//! An agent is born clean only when its direct parent, the pinned
//! factory, ran `new`. A plain deploy never is.

use agent_core::*;

const T0: Ts = 1_767_225_600 * NANOS_PER_SEC;
const FACTORY: &str = "factory.testnet";

fn runner(m: &mut Machine) {
    let deleted = m.set_runner_key("alice", "ed25519:r", T0).unwrap();
    m.runner_key_done(m.key_batch, &[], &deleted, "ed25519:r", true);
}

#[test]
fn birth_rule_needs_the_direct_parent_to_be_the_factory() {
    let mut m = Machine::new("translator.factory.testnet", "alice");
    assert!(!m.born_clean, "safe default: not born clean");
    m.mark_birth("factory.testnet", FACTORY);
    assert!(m.born_clean);
    // A plain deploy: the account called init itself.
    let mut m = Machine::new("cheat.testnet", "alice");
    m.mark_birth("cheat.testnet", FACTORY);
    assert!(!m.born_clean);
    // Same name under the factory, but init called by itself (not possible
    // on chain; the rule must still say no).
    let mut m = Machine::new("a.factory.testnet", "alice");
    m.mark_birth("a.factory.testnet", FACTORY);
    assert!(!m.born_clean);
    // A different parent, even one that called init.
    let mut m = Machine::new("a.other.testnet", "alice");
    m.mark_birth("other.testnet", FACTORY);
    assert!(!m.born_clean);
    // Two levels below the factory: the parent is not the factory.
    let mut m = Machine::new("x.a.factory.testnet", "alice");
    m.mark_birth("a.factory.testnet", FACTORY);
    assert!(!m.born_clean);
}

#[test]
fn list_is_refused_when_not_born_clean() {
    let mut m = Machine::new("cheat.testnet", "alice");
    runner(&mut m);
    let e = m
        .list("alice", 1_000 * USD, Terms::default(), "hash", T0)
        .unwrap_err();
    assert_eq!(e, Error::NotBornClean);
    assert_eq!(e.to_string(), "NotBornClean");
    assert!(m.listing.is_none());
    assert_eq!(m.status, Status::Active);
}

#[test]
fn list_works_when_born_clean_and_pay_from_revenue_too() {
    let mut m = Machine::new_born("translator", "alice");
    runner(&mut m);
    m.list("alice", 1_000 * USD, Terms::default(), "hash", T0).unwrap();
    m.cancel_listing("alice").unwrap();
    let t = Terms { earnout_bps: MAX_EARNOUT_BPS, ..Terms::default() };
    m.list("alice", 1_000 * USD, t, "hash", T0).unwrap();
}

#[test]
fn buy_is_refused_when_not_born_clean_even_if_a_listing_exists() {
    // Defence in depth: force a listing onto a not-born-clean machine (state
    // from before the rule, or a bug) and the buy still refuses.
    let mut m = Machine::new_born("translator", "alice");
    runner(&mut m);
    m.list("alice", 1_000 * USD, Terms::default(), "hash", T0).unwrap();
    m.born_clean = false;
    let h = m.known_keys_hash();
    let e = m
        .begin_buy("bob", 1_000 * USD, 1, &h, "ed25519:b", true, T0)
        .unwrap_err();
    assert_eq!(e, Error::NotBornClean);
    assert_eq!(m.status, Status::Listed, "nothing changed");
    assert_eq!(m.held, 0);
}

#[test]
fn share_offers_stay_allowed_without_a_clean_birth() {
    let mut m = Machine::new("cheat.testnet", "alice");
    runner(&mut m);
    assert!(!m.born_clean);
    // Investing never moves the deed or the keys.
    m.offer_share("alice", 1_000, 10 * USD, T0 + 30 * DAY_NS, T0)
        .expect("share offers are not a sale of the deed");
}
