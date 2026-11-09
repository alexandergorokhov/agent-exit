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
    /// SC21: Merkle root (hex) over the bundle's `CHUNK`-byte chunks, so a
    /// buyer can check random chunks before paying (`Preview`). Optional:
    /// a commitment without it sells as before, with no preview.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub root: Option<String>,
}

impl ConfigCommit {
    pub fn valid(&self) -> bool {
        let hex64 = |h: &str| {
            h.len() == 64
                && h.bytes()
                    .all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase())
        };
        (1..=MAX_CONFIG_BYTES).contains(&self.bytes)
            && hex64(&self.hash)
            && self.root.as_deref().is_none_or(hex64)
    }

    /// The commitment to `bundle`, with its chunk root (SC21).
    pub fn of(bundle: &[u8]) -> ConfigCommit {
        ConfigCommit {
            hash: hex::encode(Sha256::digest(bundle)),
            bytes: bundle.len() as u32,
            root: Some(hex::encode(chunk_root(bundle))),
        }
    }

    /// The same without a chunk root (no preview possible).
    pub fn without_root(bundle: &[u8]) -> ConfigCommit {
        ConfigCommit {
            root: None,
            ..ConfigCommit::of(bundle)
        }
    }

    /// Do these bytes match every part of the commitment?
    pub fn matches(&self, plain: &[u8]) -> bool {
        plain.len() as u32 == self.bytes
            && hex::encode(Sha256::digest(plain)) == self.hash
            && self
                .root
                .as_deref()
                .is_none_or(|r| r == hex::encode(chunk_root(plain)))
    }

    /// Number of chunks the bundle has.
    pub fn chunks(&self) -> u32 {
        self.bytes.div_ceil(CHUNK)
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

// ------------------------------------------------- SC21: chunk preview

/// Chunk size for the preview Merkle tree.
pub const CHUNK: u32 = 256;
/// Chunks a preview asks for (fewer if the bundle is smaller).
pub const PREVIEW_CHUNKS: u32 = 2;
const LEAF: &[u8] = b"agent-exit/sealed-chunk/v1";

fn leaf(index: u32, chunk: &[u8]) -> [u8; 32] {
    Sha256::digest([LEAF, &index.to_le_bytes()[..], chunk].concat()).into()
}

fn node(l: &[u8; 32], r: &[u8; 32]) -> [u8; 32] {
    Sha256::digest([&[1u8][..], l, r].concat()).into()
}

fn levels(bundle: &[u8]) -> Vec<Vec<[u8; 32]>> {
    let mut level: Vec<[u8; 32]> = bundle
        .chunks(CHUNK as usize)
        .enumerate()
        .map(|(i, c)| leaf(i as u32, c))
        .collect();
    if level.is_empty() {
        level.push(leaf(0, &[]));
    }
    let mut out = vec![level.clone()];
    while level.len() > 1 {
        // An odd last node moves up unchanged.
        level = level
            .chunks(2)
            .map(|p| {
                if p.len() == 2 {
                    node(&p[0], &p[1])
                } else {
                    p[0]
                }
            })
            .collect();
        out.push(level.clone());
    }
    out
}

/// Merkle root over the bundle's 256-byte chunks (leaf = sha256(domain ||
/// index LE || chunk), node = sha256(0x01 || left || right)).
pub fn chunk_root(bundle: &[u8]) -> [u8; 32] {
    *levels(bundle)
        .last()
        .and_then(|l| l.first())
        .expect("non-empty")
}

/// The chunk at `index` and its sibling path (hex), for `answer_preview`.
pub fn chunk_proof(bundle: &[u8], index: u32) -> Option<(Vec<u8>, Vec<String>)> {
    let chunk = bundle.chunks(CHUNK as usize).nth(index as usize)?.to_vec();
    let mut proof = Vec::new();
    let mut i = index as usize;
    for level in levels(bundle).iter().take_while(|l| l.len() > 1) {
        let sib = i ^ 1;
        if sib < level.len() {
            proof.push(hex::encode(level[sib]));
        }
        i /= 2;
    }
    Some((chunk, proof))
}

/// Check one revealed chunk against the committed root. The tree shape
/// follows from `bytes`, so a proof of the wrong length is refused.
pub fn verify_chunk(commit: &ConfigCommit, index: u32, chunk: &[u8], proof: &[String]) -> bool {
    let Some(root) = commit.root.as_deref().and_then(hex32) else {
        return false;
    };
    let n = commit.chunks();
    if index >= n {
        return false;
    }
    let want = if index + 1 == n {
        commit.bytes - index * CHUNK
    } else {
        CHUNK
    };
    if chunk.len() as u32 != want {
        return false;
    }
    let mut h = leaf(index, chunk);
    let (mut i, mut width, mut used) = (index as usize, n as usize, 0usize);
    while width > 1 {
        let sib = i ^ 1;
        if sib < width {
            let Some(s) = proof.get(used).and_then(|p| hex32(p)) else {
                return false;
            };
            used += 1;
            h = if i % 2 == 0 {
                node(&h, &s)
            } else {
                node(&s, &h)
            };
        }
        i /= 2;
        width = width.div_ceil(2);
    }
    used == proof.len() && h == root
}

/// Previews kept per listing version: one per buyer, at most this many.
pub const MAX_PREVIEWS: usize = 4;

/// How many chunks a preview of a `bytes`-byte bundle shows: at most
/// `PREVIEW_CHUNKS`, and never more than a quarter of its bytes (review 5
/// P3: counted in bytes, not chunks), so a preview cannot give the config
/// away. Zero (no preview) under 1 KB.
pub fn preview_size(bytes: u32) -> u32 {
    PREVIEW_CHUNKS.min(bytes / (4 * CHUNK))
}

/// Which chunks a preview asks for: `preview_size(bytes)` distinct indices
/// drawn from `seed` (the block's random seed, so the seller cannot pick).
pub fn preview_indices(seed: &[u8; 32], bytes: u32) -> Vec<u32> {
    let want = preview_size(bytes);
    let chunks = bytes.div_ceil(CHUNK).max(1);
    let mut out: Vec<u32> = Vec::new();
    let mut ctr = 0u32;
    while (out.len() as u32) < want {
        let h = Sha256::digest([&seed[..], &ctr.to_le_bytes()[..]].concat());
        let i = u32::from_le_bytes([h[0], h[1], h[2], h[3]]) % chunks;
        if !out.contains(&i) {
            out.push(i);
        }
        ctr += 1;
    }
    out.sort();
    out
}

/// SC21: a buyer's request to see random chunks of the listed bundle, and
/// the chunks the seller proved against the committed root.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, BorshSerialize, BorshDeserialize)]
#[cfg_attr(not(target_arch = "wasm32"), derive(borsh::BorshSchema))]
pub struct Preview {
    pub listing_version: u64,
    pub requested_by: String,
    pub requested_at: Ts,
    pub indices: Vec<u32>,
    /// (index, chunk bytes) proven against the root.
    pub proven: Vec<(u32, Vec<u8>)>,
}

impl Preview {
    pub fn complete(&self) -> bool {
        self.proven.len() == self.indices.len()
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
    // SC21: the delivered bytes must match the hash AND the chunk root, so
    // a preview of one bundle cannot sell another.
    match open(sk, eph_pk, cipher) {
        Some(plain) => commit.matches(&plain),
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
