//! Agent Exit Base wallet client (feature 5).
//!
//! - `derive`: the Chain Signatures derivation the MPC signer uses for a
//!   secp256k1 key: `epsilon = sha3_256("near-mpc-recovery v0.1.0 epsilon
//!   derivation:" + account + "," + path)`, child = root + epsilon·G, address
//!   = keccak256(child)[12..]. Gate G2 checks it against chainsig.js and the
//!   live signer `v1.signer-prod.testnet`.
//! - `MockMpc`: an offline MPC signer with a fixed TEST root key. It answers
//!   `sign` exactly like the real one (`big_r`, `s`, `recovery_id`), so the
//!   contract's callback runs unchanged in the mocked runtime.
//! - `check_auth`: recompute a `base_auth_signed` event's digest and calldata
//!   from its fields and recover the signer: what a relayer checks before it
//!   broadcasts `transferWithAuthorization`.

use agent_core::base::{self as b, keccak};
use k256::ecdsa::{RecoveryId, Signature, SigningKey, VerifyingKey};
use k256::elliptic_curve::ops::Reduce;
use k256::elliptic_curve::sec1::ToEncodedPoint;
use k256::{ProjectivePoint, PublicKey, Scalar, U256};
use serde_json::{json, Value};
use sha3::{Digest, Sha3_256};

pub const EPSILON_PREFIX: &str = "near-mpc-recovery v0.1.0 epsilon derivation:";

pub fn epsilon(account: &str, path: &str) -> Scalar {
    let h: [u8; 32] = Sha3_256::digest(format!("{EPSILON_PREFIX}{account},{path}")).into();
    <Scalar as Reduce<U256>>::reduce_bytes(&h.into())
}

/// `secp256k1:<base58 of the 64-byte x||y>` (the signer's `public_key` view).
pub fn parse_near_secp_pk(s: &str) -> Result<PublicKey, String> {
    let b58 = s
        .strip_prefix("secp256k1:")
        .ok_or("expected secp256k1:<base58>")?;
    let raw = bs58::decode(b58).into_vec().map_err(|e| e.to_string())?;
    if raw.len() != 64 {
        return Err(format!("expected 64 bytes, got {}", raw.len()));
    }
    let mut sec1 = vec![4u8];
    sec1.extend_from_slice(&raw);
    PublicKey::from_sec1_bytes(&sec1).map_err(|e| e.to_string())
}

pub fn near_secp_pk(pk: &PublicKey) -> String {
    let p = pk.to_encoded_point(false);
    format!(
        "secp256k1:{}",
        bs58::encode(&p.as_bytes()[1..]).into_string()
    )
}

pub fn address(pk: &PublicKey) -> String {
    let p = pk.to_encoded_point(false);
    format!("0x{}", hex::encode(&keccak(&p.as_bytes()[1..])[12..]))
}

/// The derived child public key of (`account`, `path`) under `root`.
pub fn derive(root: &PublicKey, account: &str, path: &str) -> PublicKey {
    let p = root.to_projective() + ProjectivePoint::GENERATOR * epsilon(account, path);
    PublicKey::from_affine(p.to_affine()).expect("not identity")
}

pub fn derive_address(root: &PublicKey, account: &str, path: &str) -> String {
    address(&derive(root, account, path))
}

/// Offline MPC signer. TEST key only; never holds real funds.
pub struct MockMpc {
    root: Scalar,
    /// Return high-s signatures (the real signer may), to test normalisation.
    pub high_s: bool,
}

impl Default for MockMpc {
    fn default() -> Self {
        MockMpc::new([0x42; 32])
    }
}

impl MockMpc {
    pub fn new(seed: [u8; 32]) -> Self {
        let root = <Scalar as Reduce<U256>>::reduce_bytes(&seed.into());
        MockMpc {
            root,
            high_s: false,
        }
    }

    /// The same TEST signer, answering with high-s signatures.
    pub fn high_s(mut self) -> Self {
        self.high_s = true;
        self
    }

    pub fn public_key(&self) -> PublicKey {
        PublicKey::from_affine((ProjectivePoint::GENERATOR * self.root).to_affine()).expect("pk")
    }

    pub fn derive_address(&self, account: &str, path: &str) -> String {
        derive_address(&self.public_key(), account, path)
    }

    /// The MPC `sign` answer for `payload` signed by (`account`, `path`).
    pub fn sign(&self, account: &str, path: &str, payload: &[u8; 32]) -> Value {
        let sk = self.root + epsilon(account, path);
        let key = SigningKey::from_bytes(&sk.to_bytes()).expect("non-zero");
        let (sig, rid) = key.sign_prehash_recoverable(payload).expect("sign");
        let (r, s) = sig.split_bytes();
        let (mut s_bytes, mut odd) = (s.to_vec(), rid.is_y_odd());
        if self.high_s {
            // (r, n - s) is the same signature with R negated: its y parity
            // flips. The real signer may answer either form.
            let s_sc = <Scalar as Reduce<U256>>::reduce_bytes(&s);
            s_bytes = (-s_sc).to_bytes().to_vec();
            odd = !odd;
        }
        let big_r = format!("{}{}", if odd { "03" } else { "02" }, hex::encode(r));
        json!({
            "scheme": "Secp256k1",
            "big_r": {"affine_point": big_r},
            "s": {"scalar": hex::encode(&s_bytes)},
            "recovery_id": odd as u8,
        })
    }
}

fn h32(v: &Value, k: &str) -> Result<[u8; 32], String> {
    let s = v[k].as_str().ok_or(format!("{k} missing"))?;
    let b = hex::decode(s.trim_start_matches("0x")).map_err(|e| format!("{k}: {e}"))?;
    b.try_into().map_err(|_| format!("{k}: not 32 bytes"))
}

fn addr(v: &Value, k: &str) -> Result<[u8; 20], String> {
    b::parse_address(v[k].as_str().ok_or(format!("{k} missing"))?)
        .ok_or(format!("{k}: bad address"))
}

fn num(v: &Value, k: &str) -> Result<u128, String> {
    match &v[k] {
        Value::String(s) => s.parse().map_err(|_| format!("{k}: not a number")),
        Value::Number(n) => n
            .as_u64()
            .map(u128::from)
            .ok_or(format!("{k}: not a number")),
        _ => Err(format!("{k} missing")),
    }
}

/// Check a `base_auth_signed` event: digest and calldata rebuilt from the
/// fields, signature low-s, signer == `from`, chain and token are Base
/// Sepolia USDC. Returns the recovered address.
pub fn check_auth(e: &Value) -> Result<String, String> {
    if e["chain_id"].as_u64() != Some(b::BASE_CHAIN_ID) {
        return Err("chain_id is not Base Sepolia (84532)".into());
    }
    if e["token"].as_str().map(str::to_ascii_lowercase).as_deref() != Some(b::BASE_USDC) {
        return Err("token is not Base Sepolia USDC".into());
    }
    let (from, to) = (addr(e, "from")?, addr(e, "to")?);
    let value = num(e, "value")?;
    let (after, before) = (
        num(e, "valid_after")? as u64,
        num(e, "valid_before")? as u64,
    );
    let nonce = h32(e, "nonce")?;
    let digest = b::eip3009_digest(&b::usdc_domain(), &from, &to, value, after, before, &nonce);
    if digest != h32(e, "digest")? {
        return Err("digest does not match the fields".into());
    }
    let (r, s) = (h32(e, "r")?, h32(e, "s")?);
    let v = e["v"].as_u64().ok_or("v missing")? as u8;
    if v != 27 && v != 28 {
        return Err("v not 27/28".into());
    }
    let sig = Signature::from_scalars(r, s).map_err(|e| e.to_string())?;
    if sig.normalize_s().is_some() {
        return Err("high-s signature (the token refuses it)".into());
    }
    let vk = VerifyingKey::recover_from_prehash(
        &digest,
        &sig,
        RecoveryId::from_byte(v - 27).ok_or("v")?,
    )
    .map_err(|e| e.to_string())?;
    let who = address(&PublicKey::from(&vk));
    if who != e["from"].as_str().unwrap_or("").to_ascii_lowercase() {
        return Err(format!("signed by {who}, not by from"));
    }
    let cd = b::relay_calldata(&from, &to, value, after, before, &nonce, v, &r, &s);
    if e["calldata"].as_str() != Some(format!("0x{}", hex::encode(cd)).as_str()) {
        return Err("calldata does not match the fields".into());
    }
    Ok(who)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn derivation_is_stable_and_per_account_and_path() {
        let m = MockMpc::default();
        let a = m.derive_address("translator.factory.testnet", "wallet-1");
        assert_eq!(
            a,
            m.derive_address("translator.factory.testnet", "wallet-1")
        );
        assert_ne!(
            a,
            m.derive_address("translator.factory.testnet", "wallet-2")
        );
        assert_ne!(a, m.derive_address("cheat.testnet", "wallet-1"));
        assert!(b::parse_address(&a).is_some());
        // Public derivation (root pk only) equals the private one.
        let pk = parse_near_secp_pk(&near_secp_pk(&m.public_key())).unwrap();
        assert_eq!(
            derive_address(&pk, "translator.factory.testnet", "wallet-1"),
            a
        );
    }

    #[test]
    fn mock_signatures_recover_to_the_derived_address() {
        for high_s in [false, true] {
            let m = MockMpc {
                high_s,
                ..Default::default()
            };
            let d = [7u8; 32];
            let r = m.sign("translator.factory.testnet", "wallet-1", &d);
            let big_r = hex::decode(r["big_r"]["affine_point"].as_str().unwrap()).unwrap();
            let s: [u8; 32] = hex::decode(r["s"]["scalar"].as_str().unwrap())
                .unwrap()
                .try_into()
                .unwrap();
            let rid = r["recovery_id"].as_u64().unwrap() as u8;
            assert_eq!(rid, (big_r[0] == 3) as u8);
            let sig =
                Signature::from_scalars(<[u8; 32]>::try_from(&big_r[1..]).unwrap(), s).unwrap();
            assert_eq!(sig.normalize_s().is_some(), high_s);
            // k256 recovers low-s only: normalise as the contract does.
            let (sig, rid) = match sig.normalize_s() {
                Some(n) => (n, rid ^ 1),
                None => (sig, rid),
            };
            let vk =
                VerifyingKey::recover_from_prehash(&d, &sig, RecoveryId::from_byte(rid).unwrap())
                    .unwrap();
            assert_eq!(
                address(&PublicKey::from(&vk)),
                m.derive_address("translator.factory.testnet", "wallet-1")
            );
        }
    }
}
