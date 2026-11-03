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
        // A51: the Base address is not the MPC derivation of the agent.
        ("base_address_mismatch", |i| {
            i.contract.base_wallet.as_mut().unwrap().address =
                Some("0x00000000000000000000000000000000000000ee".into())
        }),
        ("base_address_mismatch", |i| {
            i.contract.base_wallet.as_mut().unwrap().path = "wallet-2".into()
        }),
        ("base_address_mismatch", |i| {
            // Right address for another agent.
            let other = test_mpc().derive_address("other.testnet", BASE_PATH);
            i.contract.base_wallet.as_mut().unwrap().address = Some(other)
        }),
        ("base_wallet_unchecked", |i| i.contract.base_wallet = None),
        ("base_wallet_unchecked", |i| {
            i.contract.base_wallet.as_mut().unwrap().mpc_public_key = None
        }),
        ("base_wallet_unchecked", |i| {
            i.contract.base_wallet.as_mut().unwrap().mpc_public_key = Some("secp256k1:xyz".into())
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
    // Every case of every family is blocked, counted or not.
    let fams = r["families"].as_array().unwrap();
    assert_eq!(fams.len(), FAMILIES.len());
    let all: u64 = fams.iter().map(|f| f["cases"].as_u64().unwrap()).sum();
    assert_eq!(all, 200);
    assert!(fams
        .iter()
        .all(|f| f["missed"] == 0 && f["cases"] == f["caught"]));
    // The headline counts only counted families; A9 waits for Base.
    let a9 = fams.iter().find(|f| f["id"] == "A9").unwrap();
    assert_eq!(a9["counted"], false);
    assert_eq!(r["not_counted"][0]["id"], "A9");
    assert_eq!(r["not_counted"][0]["missed"], 0);
    let counted: Vec<_> = fams.iter().filter(|f| f["counted"] == true).collect();
    assert_eq!(counted.len(), FAMILIES.len() - NOT_COUNTED.len());
    let counted_cases: u64 = counted.iter().map(|f| f["cases"].as_u64().unwrap()).sum();
    assert_eq!(r["in_scope"]["cases"], counted_cases);
    assert_eq!(r["in_scope"]["caught"], counted_cases);
    assert_eq!(r["in_scope"]["missed"], 0);
    assert_eq!(r["summary"]["families"], counted.len());
    assert_eq!(r["summary"]["families_blocked"], counted.len());
    assert_eq!(
        r["summary"]["text"],
        format!(
            "{} attack families, each blocked by its rule; 0/50 false positives (regression suite)",
            counted.len()
        )
    );
    assert_eq!(r["clean"]["false_positives"], 0);
    assert!(FAMILIES.len() >= 16);
    // Every family gets cases; at least 20 in total even for small N.
    let small = run(1, 20, 5);
    let small_all: u64 = small["families"]
        .as_array()
        .unwrap()
        .iter()
        .map(|f| f["cases"].as_u64().unwrap())
        .sum();
    assert_eq!(small_all, 20);
    assert_eq!(small["in_scope"]["missed"], 0);
    assert_eq!(small["not_counted"][0]["missed"], 0);
}

#[test]
fn a5_uses_the_release_hash_from_sha256sums() {
    // `sha256sum` output -> NEAR's base58 code hash; other files ignored.
    let hex = "00".repeat(31) + "01";
    let sums = format!("{}  xusd.wasm\n{hex}  agent_exit.wasm\n", "ff".repeat(32));
    assert_eq!(
        code_hash_from_sums(&sums).as_deref(),
        Some("11111111111111111111111111111112")
    );
    assert_eq!(code_hash_from_sums("abc  agent_exit.wasm"), None);
    assert_eq!(
        code_hash_from_sums(&format!("{}  xusd.wasm", "ff".repeat(32))),
        None
    );
    // The corpus uses the release hash exactly when SHA256SUMS exists.
    let r = run(42, 40, 5);
    let exists = root().join(SHA256SUMS).exists();
    let (hash, source) = corpus_code_hash();
    assert_eq!(source, if exists { "release" } else { "placeholder" });
    assert_eq!(r["summary"]["code_hash"], hash);
    if !exists {
        assert_eq!(hash, FIXTURE_CODE_HASH);
    }
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

/// A51: the judge fixture's Base address is the MPC derivation (checksum
/// case does not matter); an agent without a Base wallet is not blocked.
#[test]
fn a51_base_address_is_derived_from_the_mpc_key() {
    let i = judge();
    let w = i.contract.base_wallet.clone().unwrap();
    assert_eq!(
        derive_base_address(w.mpc_public_key.as_deref().unwrap(), &i.agent, BASE_PATH).unwrap(),
        w.address.clone().unwrap()
    );
    let mut up = judge();
    up.contract.base_wallet.as_mut().unwrap().address =
        Some(w.address.unwrap().to_uppercase().replacen("0X", "0x", 1));
    assert_eq!(scan(&up).verdict, "clean");
    let mut none = judge();
    none.contract.base_wallet.as_mut().unwrap().address = None;
    assert_eq!(scan(&none).verdict, "clean");
}

/// The reproducible cargo-near build also exports its embedded ABI and the
/// NEP-330 source metadata (both read-only). Found on testnet: scanning the
/// deployed agent flagged them as unexpected methods.
#[test]
fn build_exports_are_not_unexpected_methods() {
    let mut i = judge();
    i.chain.methods.push("__contract_abi".into());
    i.chain.methods.push("contract_source_metadata".into());
    assert_eq!(scan(&i).verdict, "clean");
    i.chain.methods.push("migrate_state".into());
    assert_ne!(scan(&i).verdict, "clean");
}

/// SH7: open share slices are disclosed to the buyer, not blocked.
#[test]
fn sh7_shares_disclosed_not_blocked() {
    let mut i = judge();
    let l = i.listing.as_mut().expect("listing");
    l.shares_bps = 2_000;
    l.shares = vec![
        serde_json::json!({"offer_id": 1, "bps": 2000, "holders": [{"account": "dave.testnet", "bps": 2000}]}),
    ];
    let r = scan(&i);
    assert_eq!(r.verdict, "clean");
    let f = r
        .findings
        .iter()
        .find(|f| f.code == "shares_disclosed")
        .expect("disclosed");
    assert_eq!(f.severity, "warn");
    assert!(
        f.detail
            .starts_with("20% of every future payment goes to investors (1 slice(s), 1 holder(s))"),
        "{}",
        f.detail
    );
}

/// T8: the investor's scan drops the listing rules but keeps the key rules.
#[test]
fn t8_investor_scan_keeps_key_rules_without_a_listing() {
    let mut i = judge();
    i.listing = None;
    assert_eq!(scan(&i).verdict, "blocked");
    let r = scan_investor(&i);
    assert_eq!(r.verdict, "clean", "{:?}", r.codes());
    assert!(r.buy_args.is_none());
    i.chain.keys.push(AccessKey {
        public_key: "ed25519:hidden".into(),
        permission: Permission::Full("FullAccess".into()),
    });
    i.chain.keys_second_provider = Some(i.chain.keys.clone());
    let r = scan_investor(&i);
    assert_eq!(r.verdict, "blocked");
    assert!(r.codes().contains(&"full_access_key_present"));
}
