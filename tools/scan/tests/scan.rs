//! Scan rules and corpus (SCENARIOS.md S1 client part, E31, E34, A1-A9).

use scan::corpus::*;
use scan::*;
use std::path::Path;

fn root() -> &'static Path {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .parent()
        .unwrap()
}

fn judge() -> ScanInput {
    serde_json::from_str(
        &std::fs::read_to_string(root().join("fixtures/judge-attack.json")).unwrap(),
    )
    .unwrap()
}

#[test]
fn s1_clean_agent_scans_clean_with_buy_args() {
    let r = scan(&judge());
    assert_eq!(r.verdict, "clean");
    assert!(r.findings.is_empty());
    let a = r.buy_args.unwrap();
    assert_eq!(a["listing_version"], 1);
    assert_eq!(a["known_keys_hash"], judge().contract.known_keys_hash);
    assert_eq!(a["price"], "1000000000");
}

/// A32: a buyer may ask for more than the published minimum, and a demo may
/// accept a shorter window only by asking for it explicitly.
#[test]
fn a32_buyer_minimum_terms_are_configurable() {
    let mut i = judge();
    i.expected.min_terms = Some(MinTerms {
        min_earnout_bps: 4_000,
        ..PUBLISHED_MIN_TERMS
    });
    assert!(scan(&i).codes().contains(&"terms_below_minimum"));
    let mut i = judge();
    i.listing.as_mut().unwrap().terms.window_secs = 600;
    assert_eq!(scan(&i).verdict, "blocked");
    i.expected.min_terms = Some(MinTerms {
        min_window_secs: 600,
        ..PUBLISHED_MIN_TERMS
    });
    assert_eq!(scan(&i).verdict, "clean");
}

#[test]
fn judge_fixture_flips_when_a_key_is_planted() {
    let mut i = judge();
    i.chain.keys.push(AccessKey {
        public_key: "ed25519:planted".into(),
        permission: Permission::Full("FullAccess".into()),
    });
    let r = scan(&i);
    assert_eq!(r.verdict, "inconclusive", "providers now disagree");
    i.chain.keys_second_provider = Some(i.chain.keys.clone());
    let r = scan(&i);
    assert_eq!(r.verdict, "blocked");
    assert_eq!(r.codes(), vec!["full_access_key_present"]);
    assert!(r.buy_args.is_none());
}

#[test]
fn e31_rpc_disagree_is_inconclusive_and_refuses() {
    let mut i = judge();
    i.chain.keys_second_provider = Some(vec![]);
    let r = scan(&i);
    assert_eq!(r.verdict, "inconclusive");
    assert!(r.blocks());
    assert!(r.codes().contains(&"rpc_disagree"));
}

#[test]
fn e34_subaccount_warns_but_does_not_block() {
    let mut i = judge();
    i.chain.subaccounts = vec!["data.translator.testnet".into()];
    let r = scan(&i);
    assert_eq!(r.verdict, "clean");
    assert_eq!(r.findings[0].code, "subaccount_does_not_move");
    assert_eq!(r.findings[0].severity, "warn");
}

#[test]
fn each_rule_blocks_alone() {
    type M = fn(&mut ScanInput);
    let cases: Vec<(&str, M)> = vec![
        ("code_hash_mismatch", |i| i.chain.code_hash = "other".into()),
        ("upgrade_method", |i| i.chain.methods.push("upgrade".into())),
        ("upgrade_method", |i| {
            i.chain.methods.push("deploy_self".into())
        }),
        ("unexpected_method", |i| {
            i.chain.methods.push("drain".into())
        }),
        ("open_approval", |i| i.contract.open_approvals = 1),
        ("listing_version_mismatch", |i| {
            i.expected.listing_version = 2
        }),
        ("not_listed", |i| i.contract.status = "Sold".into()),
        ("open_base_auth", |i| {
            i.contract.open_auth_until = i.now_ns + 1
        }),
        ("frozen", |i| i.contract.frozen = true),
        ("registry_mismatch", |i| {
            i.contract.admin = Some("alice.testnet".into())
        }),
        ("registry_mismatch", |i| {
            i.contract.token = Some("fake-usd.testnet".into())
        }),
        ("registry_mismatch", |i| i.contract.token = None),
        ("known_keys_hash_mismatch", |i| {
            i.contract.known_keys_hash = "00".into()
        }),
        // A32: terms and the quoted listing.
        ("listing_missing", |i| i.listing = None),
        ("terms_below_minimum", |i| {
            i.listing.as_mut().unwrap().terms.earnout_bps = 0
        }),
        ("terms_below_minimum", |i| {
            i.listing.as_mut().unwrap().terms.floor_bps = 10_000
        }),
        ("terms_below_minimum", |i| {
            i.listing.as_mut().unwrap().terms.window_secs = 60
        }),
        ("price_changed", |i| i.expected.price = Some("1".into())),
        // A36: the buyer would inherit a full round-trip table.
        ("paid_to_bloated", |i| {
            i.contract.paid_to_total = Some(2_001)
        }),
        ("paid_to_bloated", |i| i.contract.paid_to_total = None),
        // A46: the NEAR balance does not cover storage.
        ("near_storage_insolvent", |i| {
            i.contract.near_storage_solvent = Some(false)
        }),
        ("near_storage_insolvent", |i| {
            i.contract.near_storage_solvent = None
        }),
        ("listing_version_mismatch", |i| {
            i.listing.as_mut().unwrap().listing_version = 2
        }),
    ];
    for (code, m) in cases {
        let mut i = judge();
        m(&mut i);
        let r = scan(&i);
        assert_eq!(r.verdict, "blocked", "{code}");
        assert!(r.codes().contains(&code), "{code}: {:?}", r.codes());
    }
    // An auth that expired exactly now is not open.
    let mut i = judge();
    i.contract.open_auth_until = i.now_ns;
    assert_eq!(scan(&i).verdict, "clean");
}

#[test]
fn published_methods_match_the_contract_source() {
    let src = std::fs::read_to_string(root().join("contracts/agent/src/lib.rs")).unwrap();
    let start = src.find("#[near]\nimpl AgentExit {").expect("near impl");
    let body = &src[start..];
    let end = body.find("\n}\n").unwrap();
    let mut found: Vec<String> = body[..end]
        .lines()
        .filter_map(|l| l.strip_prefix("    pub fn "))
        .map(|l| l.split(['(', '<']).next().unwrap().to_string())
        .collect();
    let mut want: Vec<String> = PUBLISHED_METHODS.iter().map(|s| s.to_string()).collect();
    found.sort();
    want.sort();
    assert_eq!(
        found, want,
        "update PUBLISHED_METHODS when the contract's methods change"
    );
    for m in PUBLISHED_METHODS {
        assert!(
            !UPGRADE_WORDS.iter().any(|w| m.contains(w)),
            "{m} looks like an upgrade method"
        );
    }
}

#[test]
fn corpus_is_seeded_and_complete() {
    assert_eq!(run(42, 200, 50), run(42, 200, 50));
    assert_ne!(run(42, 200, 50), run(7, 200, 50));
    let r = run(42, 200, 50);
    assert_eq!(r["in_scope"]["caught"], 200);
    assert_eq!(r["clean"]["false_positives"], 0);
    assert!(FAMILIES.len() >= 16);
    // Every family gets cases; at least 20 in total even for small N.
    let small = run(1, 20, 5);
    assert_eq!(small["in_scope"]["cases"], 20);
    assert_eq!(small["in_scope"]["missed"], 0);
}

#[test]
fn not_caught_list_is_documented() {
    let doc = std::fs::read_to_string(root().join("docs/NOT-CAUGHT.md")).unwrap();
    for (id, _, _) in NOT_CAUGHT {
        assert!(
            doc.contains(&format!("| {id} |")),
            "{id} missing from docs/NOT-CAUGHT.md"
        );
    }
    for w in ["prompts", "API keys", "fresh"] {
        assert!(doc.contains(w), "{w}");
    }
}

#[test]
fn a26_published_admin_and_token_match_the_contract() {
    let src = std::fs::read_to_string(root().join("contracts/agent/src/lib.rs")).unwrap();
    assert!(src.contains(&format!("None => \"{PUBLISHED_ADMIN}\"")));
    assert!(src.contains(&format!("None => \"{PUBLISHED_TOKEN}\"")));
}
