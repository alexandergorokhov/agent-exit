//! G2 (sign half), offline: every MPC-signed Base Sepolia USDC authorization
//! recorded in out/gates/g2-sign.json recovers, from its own fields (EIP-712
//! digest rebuilt here) and signature, to the agent's derived Base address.
//! A changed amount or payee no longer recovers to it.

use serde_json::Value;

fn record() -> Value {
    let p = concat!(env!("CARGO_MANIFEST_DIR"), "/../../out/gates/g2-sign.json");
    serde_json::from_str(
        &std::fs::read_to_string(p).expect("out/gates/g2-sign.json (make gate-g2)"),
    )
    .expect("json")
}

#[test]
fn g2_recorded_signatures_recover_to_the_derived_address() {
    let r = record();
    let derived = r["derived_address"]
        .as_str()
        .expect("derived_address")
        .to_ascii_lowercase();
    assert_eq!(
        r["contract_address"]
            .as_str()
            .map(str::to_ascii_lowercase)
            .as_deref(),
        Some(derived.as_str()),
        "the contract's Base wallet is the MPC-derived address"
    );
    let auths: Vec<&Value> = r["samples"]
        .as_array()
        .expect("samples")
        .iter()
        .filter_map(|s| s.get("auth").filter(|a| !a.is_null()))
        .collect();
    assert!(
        !auths.is_empty(),
        "at least one signed authorization recorded"
    );
    for a in auths {
        assert_eq!(
            base_wallet::check_auth(a)
                .expect("valid authorization")
                .to_ascii_lowercase(),
            derived
        );
    }
}

#[test]
fn g2_a_changed_authorization_does_not_recover_to_the_wallet() {
    let r = record();
    let derived = r["derived_address"].as_str().unwrap().to_ascii_lowercase();
    let a = r["samples"]
        .as_array()
        .unwrap()
        .iter()
        .find_map(|s| s.get("auth").filter(|a| !a.is_null()))
        .expect("a signed authorization")
        .clone();
    for (field, v) in [
        ("value", Value::from("999999999")),
        (
            "to",
            Value::from("0x00000000000000000000000000000000000000a1"),
        ),
    ] {
        let mut t = a.clone();
        t[field] = v;
        let got = base_wallet::check_auth(&t).map(|s| s.to_ascii_lowercase());
        assert!(
            got.as_deref() != Ok(derived.as_str()),
            "changing {field} must break the signature"
        );
    }
}
