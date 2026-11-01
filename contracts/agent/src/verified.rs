//! NEAR Verified badge: a payment may carry the NEAR AI Cloud signature of
//! the answer it pays for. The contract checks it on-chain:
//!
//! - signed text = `{model}:{sha256_hex(request)}:{response_sha256}`
//!   (NEAR AI Cloud `/v1/signature`, ecdsa, provider form);
//! - EIP-191 personal_sign hash: `keccak256("\x19Ethereum Signed Message:\n" + len + text)`;
//! - `env::ecrecover` -> 64-byte public key -> address = keccak256(pk)[12..];
//! - the address must be in the admin-set signer list (not a constant: the
//!   NEAR AI signer may rotate, gate G6);
//! - the request (JSON, at most 3 KB) must name this agent, this job, this
//!   payer and this amount in `metadata.agent` / `metadata.job_id` /
//!   `metadata.payer` / `metadata.amount` (base units, a string), so one
//!   signature cannot verify a bigger payment or a payment by someone else
//!   (A27);
//! - each signature verifies at most once (id = sha256(r || s), so flipping
//!   `v` does not make a new id; high-s signatures are refused by ecrecover's
//!   malleability flag).
//!
//! A payment whose proof fails still counts (if not flagged); it is only not
//! verified, with a reason (E11-E13, A13).

use near_sdk::env;
use near_sdk::serde::Deserialize;
use std::collections::BTreeSet;

pub const MAX_REQUEST_LEN: usize = 3_072;

#[derive(Deserialize, Debug, Clone)]
#[serde(crate = "near_sdk::serde")]
pub struct NearAiProof {
    pub model: String,
    /// The exact request body that was sent to NEAR AI Cloud.
    pub request: String,
    /// Hex sha256 of the exact response body.
    pub response_sha256: String,
    /// Hex `r || s || v`, 65 bytes, v in {0, 1, 27, 28}.
    pub signature: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Outcome {
    Verified { sig_id: [u8; 32], signer: String },
    NotVerified(&'static str),
}

pub fn signed_text(model: &str, request: &str, response_sha256: &str) -> String {
    format!(
        "{model}:{}:{response_sha256}",
        hex_lower(&env::sha256(request.as_bytes()))
    )
}

pub fn eip191_hash(text: &str) -> [u8; 32] {
    let mut m = format!("\x19Ethereum Signed Message:\n{}", text.len()).into_bytes();
    m.extend_from_slice(text.as_bytes());
    env::keccak256_array(&m)
}

/// Recover the `0x`-prefixed lowercase Ethereum address that signed `hash`.
pub fn recover_address(hash: &[u8; 32], sig: &[u8]) -> Option<String> {
    if sig.len() != 65 {
        return None;
    }
    let v = match sig[64] {
        0 | 27 => 0,
        1 | 28 => 1,
        _ => return None,
    };
    let pk = env::ecrecover(hash, &sig[..64], v, true)?;
    let h = env::keccak256_array(pk);
    Some(format!("0x{}", hex_lower(&h[12..])))
}

pub fn hex_lower(b: &[u8]) -> String {
    const H: &[u8; 16] = b"0123456789abcdef";
    let mut s = String::with_capacity(b.len() * 2);
    for x in b {
        s.push(H[(x >> 4) as usize] as char);
        s.push(H[(x & 15) as usize] as char);
    }
    s
}

pub fn hex_decode(s: &str) -> Option<Vec<u8>> {
    let s = s.strip_prefix("0x").unwrap_or(s);
    if !s.len().is_multiple_of(2) {
        return None;
    }
    (0..s.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(s.get(i..i + 2)?, 16).ok())
        .collect()
}

#[allow(clippy::too_many_arguments)]
pub fn check(
    p: &NearAiProof,
    agent: &str,
    payer: &str,
    amount: u128,
    job_id: Option<&str>,
    signers: &BTreeSet<String>,
    used: &BTreeSet<[u8; 32]>,
) -> Outcome {
    use Outcome::NotVerified as No;
    let Some(job_id) = job_id else {
        return No("NoJobId");
    };
    if p.request.len() > MAX_REQUEST_LEN {
        return No("RequestTooLong");
    }
    let resp_ok = p.response_sha256.len() == 64
        && p.response_sha256
            .bytes()
            .all(|c| c.is_ascii_digit() || (b'a'..=b'f').contains(&c));
    if !resp_ok {
        return No("BadResponseHash");
    }
    let req: near_sdk::serde_json::Value = match near_sdk::serde_json::from_str(&p.request) {
        Ok(v) => v,
        Err(_) => return No("BadRequest"),
    };
    if req["metadata"]["agent"] != agent {
        return No("WrongAgent");
    }
    if req["metadata"]["job_id"] != job_id {
        return No("WrongJob");
    }
    if req["metadata"]["payer"] != payer {
        return No("WrongPayer");
    }
    if req["metadata"]["amount"] != amount.to_string().as_str() {
        return No("WrongAmount");
    }
    let Some(sig) = hex_decode(&p.signature) else {
        return No("BadSignature");
    };
    let hash = eip191_hash(&signed_text(&p.model, &p.request, &p.response_sha256));
    let Some(signer) = recover_address(&hash, &sig) else {
        return No("BadSignature");
    };
    if !signers.contains(&signer) {
        return No("UnknownSigner");
    }
    let sig_id = env::sha256_array(&sig[..64]);
    if used.contains(&sig_id) {
        return No("SignatureUsed");
    }
    Outcome::Verified { sig_id, signer }
}
