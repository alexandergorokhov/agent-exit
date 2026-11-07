//! Sealed agent config (SC1-SC12): the part of an agent that is not on
//! chain (system prompt, tool setup, which API keys it needs) is sold as a
//! sealed bundle, in a fair exchange the contract enforces.
//!
//! 1. Before listing, the owner commits to the bundle: sha256 of its bytes
//!    and its length (`ConfigCommit`). The listing shows the commitment.
//! 2. The buyer sends a one-time X25519 public key with the buy.
//! 3. Within `DELIVER_WINDOW_NS` the seller posts the bundle encrypted to
//!    that key (`seal`). Until then no earn-out is released; if it never
//!    comes, the buyer takes the whole remaining escrow back.
//! 4. For `DISPUTE_WINDOW_NS` after delivery the buyer may reveal the
//!    one-time secret key. The contract checks it against the public key,
//!    decrypts on-chain and hashes: if the bytes are not the committed
//!    bundle, the remaining escrow goes back to the buyer. If they are, the
//!    dispute is refused (and the buyer has only exposed this one bundle).
//!
//! What it does not do: the seller has seen the bundle and can keep a copy
//! (N1), and API keys inside it stay valid until the buyer rotates them. The
//! bundle names what to rotate; the earn-out prices the rest in.
//!
//! The cipher is X25519 + a SHA-256 keystream, chosen so the contract can
//! run it with `env::sha256`-class cost: key = sha256(DOMAIN || shared ||
//! ephemeral_pk || recipient_pk), block i = sha256(key || i as u32 LE),
//! ciphertext = plaintext XOR keystream. Integrity comes from the committed
//! hash, not a MAC: the only question ever asked is "are these the
//! committed bytes?".
use crate::{Amount, Ts, DAY_NS};
use borsh::{BorshDeserialize, BorshSerialize};
use curve25519_dalek::montgomery::MontgomeryPoint;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

pub const DOMAIN: &[u8] = b"agent-exit/sealed-config/v1";
/// Largest bundle: a system prompt and a tool list, not model weights.
pub const MAX_CONFIG_BYTES: u32 = 8_192;
pub const DELIVER_WINDOW_NS: u64 = 2 * DAY_NS;
pub const DISPUTE_WINDOW_NS: u64 = 7 * DAY_NS;

/// What the owner commits to before listing (schema `config-commit.v1`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, BorshSerialize, BorshDeserialize)]
#[cfg_attr(
    not(target_arch = "wasm32"),
    derive(borsh::BorshSchema, schemars::JsonSchema)
)]
pub struct ConfigCommit {
    /// Hex sha256 of the bundle's bytes.
    pub hash: String,
    pub bytes: u32,
}

impl ConfigCommit {
    pub fn valid(&self) -> bool {
        (1..=MAX_CONFIG_BYTES).contains(&self.bytes)
            && self.hash.len() == 64
            && self
                .hash
                .bytes()
                .all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase())
    }

    pub fn of(bundle: &[u8]) -> ConfigCommit {
        ConfigCommit {
            hash: hex::encode(Sha256::digest(bundle)),
            bytes: bundle.len() as u32,
        }
    }
}

/// Where a sale's sealed config stands.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, BorshSerialize, BorshDeserialize)]
#[cfg_attr(not(target_arch = "wasm32"), derive(borsh::BorshSchema))]
pub enum Delivery {
    /// Waiting for the seller, until `deliver_by`.
    Awaited,
    /// Posted: the ciphertext's sha256 and the seller's ephemeral key.
    Delivered {
        ephemeral_pk: String,
        cipher_hash: String,
        at: Ts,
    },
    /// Not posted in time: the remaining escrow went back to the buyer.
    Missed { refunded: Amount },
    /// A dispute proved the bytes were not the committed bundle.
    Mismatch { refunded: Amount },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, BorshSerialize, BorshDeserialize)]
#[cfg_attr(not(target_arch = "wasm32"), derive(borsh::BorshSchema))]
pub struct SaleConfig {
    pub commit: ConfigCommit,
    /// The buyer's one-time X25519 public key (hex).
    pub delivery_pk: String,
    pub deliver_by: Ts,
    pub state: Delivery,
    /// SC14: the upfront part of the price (`Sale::paid_now`), held until the
    /// buyer accepts the delivery or the dispute window closes. A missed or
    /// disproved delivery sends it back to the buyer with the escrow.
    #[serde(default)]
    pub upfront_held: Amount,
    /// Accepted by the buyer, or undisputed when the window closed: the
    /// upfront part went to the seller and earn-out releases may start.
    #[serde(default)]
    pub cleared: bool,
    /// The held upfront part that went back to the buyer (missed or
    /// disproved delivery); `Sale::returned` counts the escrow only.
    #[serde(default)]
    pub upfront_returned: Amount,
}

impl SaleConfig {
    /// Releases to the seller wait for the bundle.
    pub fn delivered(&self) -> bool {
        matches!(self.state, Delivery::Delivered { .. })
    }

    /// End of the buyer's dispute window (0 while not delivered).
    pub fn dispute_until(&self) -> Ts {
        match self.state {
            Delivery::Delivered { at, .. } => at + DISPUTE_WINDOW_NS,
            _ => 0,
        }
    }
}

pub fn to_hex(b: &[u8]) -> String {
    hex::encode(b)
}

pub fn hex32(s: &str) -> Option<[u8; 32]> {
    let v = hex::decode(s).ok()?;
    v.try_into().ok()
}

/// The X25519 public key of a secret key (clamped, RFC 7748).
pub fn public_key(sk: &[u8; 32]) -> [u8; 32] {
    MontgomeryPoint::mul_base_clamped(*sk).to_bytes()
}

/// Can anything be sealed to `pk`? Not a low-order point: every clamped
/// scalar is a multiple of the cofactor, so one test multiplication finds
/// them all (judge 19: a buyer key the seller cannot seal to would let the
/// buyer claim the price back as "undelivered").
pub fn usable_key(pk: &[u8; 32]) -> bool {
    shared(&[9u8; 32], pk).is_some()
}

fn shared(sk: &[u8; 32], pk: &[u8; 32]) -> Option<[u8; 32]> {
    let s = MontgomeryPoint(*pk).mul_clamped(*sk).to_bytes();
    // A low-order point gives an all-zero secret that anyone can compute.
    (s != [0u8; 32]).then_some(s)
}

fn stream_key(shared: &[u8; 32], eph_pk: &[u8; 32], recipient_pk: &[u8; 32]) -> [u8; 32] {
    let mut h = Sha256::new();
    h.update(DOMAIN);
    h.update(shared);
    h.update(eph_pk);
    h.update(recipient_pk);
    h.finalize().into()
}

fn xor_stream(key: &[u8; 32], data: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(data.len());
    for (i, chunk) in data.chunks(32).enumerate() {
        let mut h = Sha256::new();
        h.update(key);
        h.update((i as u32).to_le_bytes());
        let block: [u8; 32] = h.finalize().into();
        out.extend(chunk.iter().zip(block.iter()).map(|(a, b)| a ^ b));
    }
    out
}

/// Seller side: encrypt `bundle` to `recipient_pk` with an ephemeral secret
/// `eph_sk` (fresh random bytes). Returns (ephemeral public key, ciphertext);
/// `None` if `recipient_pk` is a low-order point.
pub fn seal(
    eph_sk: &[u8; 32],
    recipient_pk: &[u8; 32],
    bundle: &[u8],
) -> Option<([u8; 32], Vec<u8>)> {
    let eph_pk = public_key(eph_sk);
    let s = shared(eph_sk, recipient_pk)?;
    Some((
        eph_pk,
        xor_stream(&stream_key(&s, &eph_pk, recipient_pk), bundle),
    ))
}

/// Buyer side (and the contract, in a dispute): decrypt. `None` if the
/// seller's ephemeral key is a low-order point (counted as a bad delivery).
pub fn open(sk: &[u8; 32], eph_pk: &[u8; 32], cipher: &[u8]) -> Option<Vec<u8>> {
    let pk = public_key(sk);
    let s = shared(sk, eph_pk)?;
    Some(xor_stream(&stream_key(&s, eph_pk, &pk), cipher))
}

/// The dispute's verdict: does this delivery open to the committed bundle?
pub fn delivery_matches(
    sk: &[u8; 32],
    eph_pk: &[u8; 32],
    cipher: &[u8],
    commit: &ConfigCommit,
) -> bool {
    match open(sk, eph_pk, cipher) {
        Some(plain) => {
            plain.len() as u32 == commit.bytes && ConfigCommit::of(&plain).hash == commit.hash
        }
        None => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rfc7748_vector() {
        // RFC 7748 section 6.1: Alice's keys.
        let sk = hex32("77076d0a7318a57d3c16c17251b26645df4c2f87ebc0992ab177fba51db92c2a").unwrap();
        assert_eq!(
            hex::encode(public_key(&sk)),
            "8520f0098930a754748b7ddcb43ef75a0dbf3a0d26381af4eba4a98eaa9b4e6a"
        );
        let bob_pk =
            hex32("de9edb7d7b7dc1b4d35b61c2ece435373f8343c85b78674dadfc7e146f882b4f").unwrap();
        assert_eq!(
            hex::encode(shared(&sk, &bob_pk).unwrap()),
            "4a5d9d5ba4ce2de1728e3bf480350f25e07e21c947d19e3376f09b3c1e161742"
        );
    }

    /// The same vector as `scripts/sealed-config.mjs` (Node's X25519): the
    /// buyer's and seller's tools and the contract agree byte for byte (SC11).
    #[test]
    fn node_tool_vector() {
        let sk = [42u8; 32];
        assert_eq!(
            hex::encode(public_key(&sk)),
            "07aaff3e9fc167275544f4c3a6a17cd837f2ec6e78cd8a57b1e3dfb3cc035a76"
        );
        let (eph, c) = seal(&[17u8; 32], &public_key(&sk), br#"{"system_prompt":"hi"}"#).unwrap();
        assert_eq!(
            hex::encode(eph),
            "7b4e909bbe7ffe44c465a220037d608ee35897d31ef972f07f74892cb0f73f13"
        );
        assert_eq!(
            hex::encode(c),
            "a20c045dd426fb7c6a52a1d1a68cee605a758b9e0cf0"
        );
    }

    #[test]
    fn seal_then_open_round_trips_and_a_wrong_key_does_not() {
        let buyer_sk = [7u8; 32];
        let bundle =
            b"{\"system_prompt\":\"translate politely\",\"rotate\":[\"NEARAI_API_KEY\"]}".repeat(5);
        let (eph, c) = seal(&[9u8; 32], &public_key(&buyer_sk), &bundle).unwrap();
        assert_eq!(c.len(), bundle.len());
        assert_ne!(c, bundle);
        assert_eq!(open(&buyer_sk, &eph, &c).unwrap(), bundle);
        assert!(delivery_matches(
            &buyer_sk,
            &eph,
            &c,
            &ConfigCommit::of(&bundle)
        ));
        assert!(!delivery_matches(
            &[8u8; 32],
            &eph,
            &c,
            &ConfigCommit::of(&bundle)
        ));
        let mut bad = c.clone();
        bad[3] ^= 1;
        assert!(!delivery_matches(
            &buyer_sk,
            &eph,
            &bad,
            &ConfigCommit::of(&bundle)
        ));
        // A low-order ephemeral key opens to nothing.
        assert!(open(&buyer_sk, &[0u8; 32], &c).is_none());
    }
}
