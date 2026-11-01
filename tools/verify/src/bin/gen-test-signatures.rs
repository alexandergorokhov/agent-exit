//! Writes `fixtures/test-signatures/translator.json`: NEAR AI Cloud-shaped
//! signatures made with LOCAL TEST KEYS. They are test fixtures, not NEAR AI
//! signatures. The real NEAR AI signer is checked on day 1 (gate G6) on the
//! owner's machine and recorded separately.
//!
//! ECDSA here is deterministic (RFC 6979), so the file is reproducible:
//! `gen-test-signatures --check` fails if the committed file differs.

use k256::ecdsa::SigningKey;
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use sha3::Keccak256;

/// The test signer an agent's admin allowlists in tests. NOT a NEAR AI key.
const TEST_SIGNER_SK: [u8; 32] = [0x11; 32];
/// A second test key that is never allowlisted (forged-signer case).
const ROGUE_SK: [u8; 32] = [0x22; 32];
const MODEL: &str = "test-model/fixture-v1";

fn address(sk: &SigningKey) -> String {
    let pt = sk.verifying_key().to_encoded_point(false);
    let h = Keccak256::digest(&pt.as_bytes()[1..]);
    format!("0x{}", hex::encode(&h[12..]))
}

fn sign(sk: &SigningKey, agent: &str, job: &str, prompt: &str) -> Value {
    let request = json!({
        "model": MODEL,
        "messages": [{"role": "user", "content": prompt}],
        "metadata": {"agent": agent, "job_id": job},
    })
    .to_string();
    let response = json!({"choices": [{"message": {"role": "assistant", "content": format!("answer to {job}")}}]})
        .to_string();
    let resp_hash = hex::encode(Sha256::digest(response.as_bytes()));
    let text = format!(
        "{MODEL}:{}:{resp_hash}",
        hex::encode(Sha256::digest(request.as_bytes()))
    );
    let mut msg = format!("\x19Ethereum Signed Message:\n{}", text.len()).into_bytes();
    msg.extend_from_slice(text.as_bytes());
    let hash = Keccak256::digest(&msg);
    let (sig, rec) = sk.sign_prehash_recoverable(&hash).expect("sign");
    let mut bytes = sig.to_bytes().to_vec();
    bytes.push(27 + rec.to_byte());
    json!({
        "job_id": job, "model": MODEL, "request": request, "response": response,
        "response_sha256": resp_hash, "text": text, "signature": hex::encode(bytes),
        "signing_address": address(sk),
    })
}

fn build() -> String {
    let good = SigningKey::from_bytes(&TEST_SIGNER_SK.into()).expect("key");
    let rogue = SigningKey::from_bytes(&ROGUE_SK.into()).expect("key");
    let agent = "translator.testnet";
    let entries: Vec<Value> = (1..=10)
        .map(|i| {
            sign(
                &good,
                agent,
                &format!("job-{i}"),
                &format!("translate text {i}"),
            )
        })
        .collect();
    let v = json!({
        "schema": "nearai-sig-fixture.v1",
        "label": "TEST FIXTURE: signed with local test keys (secp256k1 0x11..11 and 0x22..22). NOT a NEAR AI Cloud signature.",
        "test_signer": address(&good),
        "rogue_signer": address(&rogue),
        "agent": agent,
        "entries": entries,
        "wrong_agent": sign(&good, "cheat.testnet", "job-x", "for another agent"),
        "rogue": sign(&rogue, agent, "job-r", "signed by a key nobody allowlisted"),
        "extra": sign(&good, agent, "job-11", "post-sale job"),
    });
    serde_json::to_string_pretty(&v).expect("json") + "\n"
}

fn main() {
    let path = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../fixtures/test-signatures/translator.json"
    );
    let text = build();
    if std::env::args().any(|a| a == "--check") {
        let have = std::fs::read_to_string(path).unwrap_or_default();
        if have != text {
            eprintln!("{path} differs from what the generator makes");
            std::process::exit(1);
        }
        eprintln!("{path} is reproducible");
    } else {
        std::fs::write(path, text).expect("write");
        eprintln!("wrote {path}");
    }
}
