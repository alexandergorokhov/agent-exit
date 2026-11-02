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
//!   malleability flag);
//! - the request carries `metadata.signed_at` (unix seconds, a string) no
//!   more than `SIG_WINDOW_SECS` old and not ahead of the block by more than
//!   `SIG_SKEW_SECS`. A used id is kept only until its proof expires, at
//!   most `MAX_SIGS` at a time, so the agent's storage for outsiders' proofs
//!   is bounded (A46). A full window means NotVerified (`SigWindowFull`);
//!   the payment still counts.
//! - a payer with its own row (registered with a NEAR deposit) has its own
//!   `PAYER_SIGS` slots, paid for by that deposit, used before the shared
//!   window; filling the shared window cannot deny their badge (A54).
//!
//! A payment whose proof fails still counts (if not flagged); it is only not
//! verified, with a reason (E11-E13, A13).

use near_sdk::env;
use near_sdk::serde::Deserialize;
use std::collections::BTreeSet;

pub const MAX_REQUEST_LEN: usize = 3_072;
/// How old a proof may be (A46).
pub const SIG_WINDOW_SECS: u64 = 7 * 86_400;
/// How far ahead of the block a proof's `signed_at` may be.
pub const SIG_SKEW_SECS: u64 = 3_600;
/// Most signature ids kept at a time in the shared window (A46).
pub const MAX_SIGS: u32 = 512;
/// A registered payer's own signature slots (A54).
pub const PAYER_SIGS: u8 = 8;

/// Used signature ids, each until its proof expires, in arrival order.
/// Bounded: at most `MAX_SIGS` entries (A46).
// `near` adds BorshSchema exactly when the ABI build needs it (the store
// collections implement it only under near-sdk's ABI feature).
#[near_sdk::near(serializers = [borsh])]
pub struct UsedSigs {
    /// sig id -> unix second after which its proof is refused anyway.
    expires: near_sdk::store::LookupMap<[u8; 32], u64>,
    /// Ring of ids: slot -> id.
    ring: near_sdk::store::LookupMap<u32, [u8; 32]>,
    head: u32,
    len: u32,
    /// Registered payers' own slots: payer ++ slot -> (id, expires) (A54).
    slots: near_sdk::store::LookupMap<Vec<u8>, ([u8; 32], u64)>,
}

impl UsedSigs {
    pub fn new(prefix: u8) -> Self {
        UsedSigs {
            expires: near_sdk::store::LookupMap::new(vec![prefix, b'e']),
            ring: near_sdk::store::LookupMap::new(vec![prefix, b'r']),
            head: 0,
            len: 0,
            slots: near_sdk::store::LookupMap::new(vec![prefix, b's']),
        }
    }

    fn slot_key(payer: &str, i: u8) -> Vec<u8> {
        let mut k = payer.as_bytes().to_vec();
        k.push(i);
        k
    }

    /// A free or expired slot of `payer`'s own (A54).
    fn payer_slot(&self, payer: &str, now_s: u64) -> Option<u8> {
        (0..PAYER_SIGS).find(|i| {
            self.slots
                .get(&Self::slot_key(payer, *i))
                .is_none_or(|(_, e)| *e < now_s)
        })
    }

    /// Room for one more id: in `payer`'s own slots (registered payers
    /// only) or in the shared window.
    pub fn has_room(&self, payer: Option<&str>, now_s: u64) -> bool {
        payer.is_some_and(|p| self.payer_slot(p, now_s).is_some()) || !self.full(now_s)
    }

    /// Record `id` in `payer`'s own slots if it has a free one, else in the
    /// shared window (call only after `check` said Verified).
    pub fn insert_for(&mut self, payer: Option<&str>, id: [u8; 32], expires: u64, now_s: u64) {
        if let Some((p, i)) = payer.and_then(|p| self.payer_slot(p, now_s).map(|i| (p, i))) {
            let k = Self::slot_key(p, i);
            if let Some((old, _)) = self.slots.get(&k).copied() {
                // Expired: it can never verify again (A46).
                self.expires.remove(&old);
            }
            self.slots.insert(k, (id, expires));
            self.expires.insert(id, expires);
            return;
        }
        self.insert(id, expires, now_s);
    }

    pub fn flush(&mut self) {
        self.expires.flush();
        self.ring.flush();
        self.slots.flush();
    }

    pub fn len(&self) -> u32 {
        self.len
    }

    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    pub fn contains(&self, id: &[u8; 32]) -> bool {
        self.expires.contains_key(id)
    }

    /// The oldest entry has expired (it can make room).
    fn head_expired(&self, now_s: u64) -> bool {
        self.len > 0
            && self
                .ring
                .get(&self.head)
                .and_then(|id| self.expires.get(id))
                .is_some_and(|e| *e < now_s)
    }

    /// No room for a new id, even after pruning.
    pub fn full(&self, now_s: u64) -> bool {
        self.len >= MAX_SIGS && !self.head_expired(now_s)
    }

    /// Drop up to `n` expired ids from the front.
    pub fn prune(&mut self, now_s: u64, n: usize) {
        for _ in 0..n {
            if !self.head_expired(now_s) {
                break;
            }
            if let Some(id) = self.ring.remove(&self.head) {
                self.expires.remove(&id);
            }
            self.head = (self.head + 1) % MAX_SIGS;
            self.len -= 1;
        }
    }

    /// Record `id` (call only after `check` said Verified, so not full).
    pub fn insert(&mut self, id: [u8; 32], expires: u64, now_s: u64) {
        self.prune(now_s, 4);
        if self.len >= MAX_SIGS {
            return;
        }
        let slot = (self.head + self.len) % MAX_SIGS;
        self.ring.insert(slot, id);
        self.expires.insert(id, expires);
        self.len += 1;
    }
}

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
    Verified {
        sig_id: [u8; 32],
        signer: String,
        /// Unix second after which this proof is refused (A46).
        expires: u64,
    },
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
    used: &UsedSigs,
    payer_lane: bool,
    now_s: u64,
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
    let signed_at = match req["metadata"]["signed_at"].as_str() {
        None => return No("NoSignedAt"),
        Some(t) if t.is_empty() || t.len() > 12 || !t.bytes().all(|c| c.is_ascii_digit()) => {
            return No("BadSignedAt")
        }
        Some(t) => t.parse::<u64>().unwrap_or(u64::MAX),
    };
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
    if signed_at > now_s.saturating_add(SIG_SKEW_SECS) {
        return No("SignedInFuture");
    }
    if signed_at.saturating_add(SIG_WINDOW_SECS) < now_s {
        return No("SignatureExpired");
    }
    if !used.has_room(payer_lane.then_some(payer), now_s) {
        return No("SigWindowFull");
    }
    Outcome::Verified {
        sig_id,
        signer,
        // Kept until no window could accept it again.
        expires: signed_at + SIG_WINDOW_SECS + SIG_SKEW_SECS,
    }
}
