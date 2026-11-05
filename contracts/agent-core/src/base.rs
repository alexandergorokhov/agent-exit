//! Feature 5, "the wallet follows the name": the agent's Base Sepolia wallet.
//!
//! The wallet is a Chain Signatures address: the MPC signer
//! (`v1.signer-prod.testnet`) derives one secp256k1 key from (this agent's
//! account, path `wallet-1`). The agent account never changes hands, so the
//! address never changes either. What changes at a sale is who can make it
//! sign: only the agent's runner key (which `buy()` swaps to the buyer's) can
//! call `pay_base`, and only inside the owner's Base limits, which belong to
//! the deed like the other limits.
//!
//! `pay_base` never signs a raw payload. It builds one USDC EIP-3009
//! `transferWithAuthorization` digest from typed fields (from = the agent's
//! address, to = an allowed payee, value, a validity of at most an hour, a
//! fresh nonce), so the MPC key can only ever authorize a capped USDC
//! transfer to an allowed payee. Any relayer broadcasts it and pays the gas.
//!
//! While any signed authorization is still valid the agent cannot be listed
//! or bought (`OpenApproval`, A9): a seller cannot pre-sign a payment that
//! lands after the sale.

use crate::policy::Spent;
use crate::{Amount, Error, Policy, Res, Ts, DAY_NS, NANOS_PER_SEC};
use borsh::{BorshDeserialize, BorshSerialize};
use serde::{Deserialize, Serialize};
use sha3::{Digest, Keccak256};
use std::collections::{BTreeMap, BTreeSet};

/// Base Sepolia.
pub const BASE_CHAIN_ID: u64 = 84_532;
/// Circle's USDC on Base Sepolia (EIP-3009). Lowercase.
pub const BASE_USDC: &str = "0x036cbd53842c5426634e7929541ec2318f3dcf7e";
/// EIP-712 domain of that token (`name()`, `version()`); gate G2 compares the
/// resulting domain separator with the token's `DOMAIN_SEPARATOR()`.
pub const USDC_NAME: &str = "USDC";
pub const USDC_VERSION: &str = "2";
/// Chain Signatures derivation path of the agent's Base wallet.
pub const BASE_PATH: &str = "wallet-1";
/// Longest validity of one authorization (E23).
pub const MAX_VALID_SECS: u64 = 3_600;
/// USDC has 6 decimals, like the test dollar.
pub const USDC: Amount = 1_000_000;
/// Highest Base daily cap an owner may set (a typo guard, E33).
pub const MAX_BASE_CAP: Amount = 10_000 * USDC;
pub const MAX_BASE_PAYEES: usize = 16;
/// Signed authorizations kept for `get_base_wallet` (older ones drop off;
/// `open_auth_until` keeps covering them).
pub const KEEP_AUTHS: usize = 20;
/// Base authorizations waiting for the MPC signer at once. Bounds the state
/// blob and the gas every callback reserves (STR-19).
pub const MAX_PENDING_AUTHS: usize = 16;

/// The owner's Base limits. Default: nothing may be paid.
#[derive(
    Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, BorshSerialize, BorshDeserialize,
)]
#[cfg_attr(not(target_arch = "wasm32"), derive(borsh::BorshSchema))]
pub struct BaseLimits {
    /// Most USDC (base units) the runner may authorize per UTC day, per owner.
    pub daily_cap: Amount,
    /// The only Base addresses the runner may pay (`0x` + 40 lowercase hex).
    pub payees: BTreeSet<String>,
}

impl BaseLimits {
    pub fn check(&self, own_address: Option<&str>) -> Res<()> {
        let ok = self.daily_cap <= MAX_BASE_CAP
            && self.payees.len() <= MAX_BASE_PAYEES
            && self.payees.iter().all(|p| parse_address(p).is_some())
            && own_address.is_none_or(|a| !self.payees.contains(a));
        if ok {
            Ok(())
        } else {
            Err(Error::BadLimits)
        }
    }
}

/// One `transferWithAuthorization` the agent asked the MPC signer to sign.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, BorshSerialize, BorshDeserialize)]
#[cfg_attr(not(target_arch = "wasm32"), derive(borsh::BorshSchema))]
pub struct BaseAuth {
    pub id: u64,
    pub owner_epoch: u64,
    pub from: String,
    pub to: String,
    pub value: Amount,
    pub valid_after: u64,
    /// Unix seconds.
    pub valid_before: u64,
    /// `0x` + 64 hex.
    pub nonce: String,
    /// The EIP-712 digest the MPC signs, `0x` + 64 hex.
    pub digest: String,
    /// UTC day the cap was charged on.
    pub day: u64,
}

#[derive(
    Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, BorshSerialize, BorshDeserialize,
)]
#[cfg_attr(not(target_arch = "wasm32"), derive(borsh::BorshSchema))]
pub struct BaseWallet {
    /// The derived address, fixed at init. The contract checks every MPC
    /// signature against it; a wrong one only ever yields `sign_failed`.
    pub address: Option<String>,
    pub limits: BaseLimits,
    pub spent: Spent,
    pub next_id: u64,
    /// Sent to the MPC signer, callback not run yet.
    pub pending: BTreeMap<u64, BaseAuth>,
    /// Signed (most recent last, at most `KEEP_AUTHS`).
    pub signed: Vec<BaseAuth>,
    /// Latest `valid_before` (ns) of every authorization ever signed,
    /// including ones dropped from `signed`.
    pub signed_until: Ts,
}

impl BaseWallet {
    pub fn spent_today(&self, owner_epoch: u64, now: Ts) -> Amount {
        self.spent.today(owner_epoch, now / DAY_NS)
    }
}

/// Plan for one `pay_base`: what to ask the MPC signer to sign.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BasePlan {
    pub auth: BaseAuth,
    pub digest: [u8; 32],
    /// Latest `valid_before` (ns) of any authorization, after this one.
    pub open_auth_until: Ts,
}

pub fn keccak(data: &[u8]) -> [u8; 32] {
    Keccak256::digest(data).into()
}

/// `0x`-prefixed 40-hex address -> 20 bytes (any case).
pub fn parse_address(a: &str) -> Option<[u8; 20]> {
    let h = a.strip_prefix("0x")?;
    if h.len() != 40 {
        return None;
    }
    let v = hex::decode(h).ok()?;
    v.try_into().ok()
}

pub fn norm_address(a: &str) -> Option<String> {
    parse_address(a.trim()).map(|b| format!("0x{}", hex::encode(b)))
}

fn word_u(n: u128) -> [u8; 32] {
    let mut w = [0u8; 32];
    w[16..].copy_from_slice(&n.to_be_bytes());
    w
}

fn word_addr(a: &[u8; 20]) -> [u8; 32] {
    let mut w = [0u8; 32];
    w[12..].copy_from_slice(a);
    w
}

/// EIP-712 domain separator of an `EIP712Domain(string name,string version,
/// uint256 chainId,address verifyingContract)` token.
pub fn domain_separator(name: &str, version: &str, chain_id: u64, contract: &[u8; 20]) -> [u8; 32] {
    let mut b = Vec::with_capacity(160);
    b.extend_from_slice(&keccak(
        b"EIP712Domain(string name,string version,uint256 chainId,address verifyingContract)",
    ));
    b.extend_from_slice(&keccak(name.as_bytes()));
    b.extend_from_slice(&keccak(version.as_bytes()));
    b.extend_from_slice(&word_u(chain_id as u128));
    b.extend_from_slice(&word_addr(contract));
    keccak(&b)
}

/// The Base Sepolia USDC domain separator.
pub fn usdc_domain() -> [u8; 32] {
    domain_separator(
        USDC_NAME,
        USDC_VERSION,
        BASE_CHAIN_ID,
        &parse_address(BASE_USDC).expect("const"),
    )
}

/// EIP-712 digest of `TransferWithAuthorization(address from,address to,
/// uint256 value,uint256 validAfter,uint256 validBefore,bytes32 nonce)`.
pub fn eip3009_digest(
    domain: &[u8; 32],
    from: &[u8; 20],
    to: &[u8; 20],
    value: Amount,
    valid_after: u64,
    valid_before: u64,
    nonce: &[u8; 32],
) -> [u8; 32] {
    let mut s = Vec::with_capacity(224);
    s.extend_from_slice(&keccak(
        b"TransferWithAuthorization(address from,address to,uint256 value,uint256 validAfter,uint256 validBefore,bytes32 nonce)",
    ));
    s.extend_from_slice(&word_addr(from));
    s.extend_from_slice(&word_addr(to));
    s.extend_from_slice(&word_u(value));
    s.extend_from_slice(&word_u(valid_after as u128));
    s.extend_from_slice(&word_u(valid_before as u128));
    s.extend_from_slice(nonce);
    let mut m = Vec::with_capacity(66);
    m.extend_from_slice(b"\x19\x01");
    m.extend_from_slice(domain);
    m.extend_from_slice(&keccak(&s));
    keccak(&m)
}

/// Nonce of authorization `id` of `agent`: unique per agent and attempt, so a
/// retry after a failed sign uses a new nonce (E22) and no two
/// authorizations can collide on the token.
pub fn auth_nonce(agent: &str, id: u64) -> [u8; 32] {
    keccak(format!("agent_exit:base:{agent}:{id}").as_bytes())
}

/// `transferWithAuthorization(address,address,uint256,uint256,uint256,bytes32,uint8,bytes32,bytes32)`
/// calldata for a relayer.
#[allow(clippy::too_many_arguments)]
pub fn relay_calldata(
    from: &[u8; 20],
    to: &[u8; 20],
    value: Amount,
    valid_after: u64,
    valid_before: u64,
    nonce: &[u8; 32],
    v: u8,
    r: &[u8; 32],
    s: &[u8; 32],
) -> Vec<u8> {
    let sel = keccak(b"transferWithAuthorization(address,address,uint256,uint256,uint256,bytes32,uint8,bytes32,bytes32)");
    let mut d = sel[..4].to_vec();
    d.extend_from_slice(&word_addr(from));
    d.extend_from_slice(&word_addr(to));
    d.extend_from_slice(&word_u(value));
    d.extend_from_slice(&word_u(valid_after as u128));
    d.extend_from_slice(&word_u(valid_before as u128));
    d.extend_from_slice(nonce);
    d.extend_from_slice(&word_u(v as u128));
    d.extend_from_slice(r);
    d.extend_from_slice(s);
    d
}

impl<S: crate::Store + Default> crate::Machine<S> {
    /// Owner's Base limits; same rules as `set_limits` (call after it).
    pub fn set_base_limits(&mut self, caller: &str, limits: BaseLimits) -> Res<()> {
        self.only_owner(caller)?;
        limits.check(self.base.address.as_deref())?;
        self.base.limits = limits;
        Ok(())
    }

    /// The runner asks for a Base payment. Call only after `runner_guard`
    /// returned `Ok`. Checks: not listed, a wallet address, validity at most
    /// an hour, an allowed payee, the Base daily cap. Reserves the cap and
    /// marks the authorization open until `valid_before` (A9) before the
    /// MPC signer is asked. Nothing changes on error.
    pub fn begin_base_pay(
        &mut self,
        to: &str,
        value: Amount,
        valid_secs: u64,
        now: Ts,
    ) -> Res<BasePlan> {
        if self.status == crate::Status::Listed {
            return Err(Error::SpendingPaused);
        }
        let from = self.base.address.clone().ok_or(Error::BaseWalletNotSet)?;
        if self.base.pending.len() >= MAX_PENDING_AUTHS {
            return Err(Error::TooManyPendingAuths);
        }
        if valid_secs == 0 || valid_secs > MAX_VALID_SECS {
            return Err(Error::ValidityTooLong);
        }
        if value == 0 {
            return Err(Error::InsufficientFunds);
        }
        let to = norm_address(to).ok_or(Error::PolicyViolation(Policy::BasePayee))?;
        if !self.base.limits.payees.contains(&to) {
            return Err(Error::PolicyViolation(Policy::BasePayee));
        }
        let day = now / DAY_NS;
        let spent = self.base.spent.today(self.owner_epoch, day);
        match spent.checked_add(value) {
            Some(t) if t <= self.base.limits.daily_cap => {}
            _ => return Err(Error::PolicyViolation(Policy::BaseDailyCap)),
        }
        let id = self.base.next_id;
        let nonce = auth_nonce(&self.agent, id);
        let valid_before = now / NANOS_PER_SEC + valid_secs;
        let from_b = parse_address(&from).expect("checked at init");
        let to_b = parse_address(&to).expect("normalised");
        let digest = eip3009_digest(
            &usdc_domain(),
            &from_b,
            &to_b,
            value,
            0,
            valid_before,
            &nonce,
        );
        let auth = BaseAuth {
            id,
            owner_epoch: self.owner_epoch,
            from,
            to,
            value,
            valid_after: 0,
            valid_before,
            nonce: format!("0x{}", hex::encode(nonce)),
            digest: format!("0x{}", hex::encode(digest)),
            day,
        };
        self.base.next_id += 1;
        self.base.spent = Spent {
            owner_epoch: self.owner_epoch,
            day,
            amount: spent + value,
        };
        self.base.pending.insert(id, auth.clone());
        self.open_auth_until = self.open_auth_until.max(valid_before * NANOS_PER_SEC);
        Ok(BasePlan {
            auth,
            digest,
            open_auth_until: self.open_auth_until,
        })
    }

    /// MPC callback. `signed`: the signature recovered to the wallet address.
    /// On failure the reserved cap is given back (same owner and day only)
    /// and the authorization is dropped; `open_auth_until` is recomputed from
    /// what is really signed or still pending (E22).
    /// C29: the signer answered with a result that cannot be checked. A
    /// signature may exist anyway, so the cap stays spent and the agent stays
    /// blocked until `valid_before`, but nothing is issued.
    pub fn finish_base_pay_unknown(&mut self, id: u64) -> Option<BaseAuth> {
        let auth = self.base.pending.remove(&id)?;
        self.base.signed_until = self
            .base
            .signed_until
            .max(auth.valid_before * NANOS_PER_SEC);
        self.open_auth_until = self.open_auth_until.max(self.base.signed_until);
        Some(auth)
    }

    pub fn finish_base_pay(&mut self, id: u64, signed: bool) -> Option<BaseAuth> {
        let auth = self.base.pending.remove(&id)?;
        if signed {
            self.base.signed_until = self
                .base
                .signed_until
                .max(auth.valid_before * NANOS_PER_SEC);
            self.base.signed.push(auth.clone());
            if self.base.signed.len() > KEEP_AUTHS {
                self.base.signed.remove(0);
            }
        } else {
            let s = &mut self.base.spent;
            if s.owner_epoch == auth.owner_epoch && s.day == auth.day {
                s.amount = s.amount.saturating_sub(auth.value);
            }
            // No signature exists for this one: the agent stays blocked only
            // by what is really signed or still pending.
            let pending = self
                .base
                .pending
                .values()
                .map(|a| a.valid_before * NANOS_PER_SEC)
                .max()
                .unwrap_or(0);
            self.open_auth_until = self.base.signed_until.max(pending);
        }
        Some(auth)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn typehashes_match_the_usdc_contract() {
        // FiatTokenV2 (USDC) constants.
        assert_eq!(
            hex::encode(keccak(b"TransferWithAuthorization(address from,address to,uint256 value,uint256 validAfter,uint256 validBefore,bytes32 nonce)")),
            "7c7c6cdb67a18743f49ec6fa9b35f50d52ed05cbed4cc592e13b44501c1a2267"
        );
        assert_eq!(
            hex::encode(keccak(b"EIP712Domain(string name,string version,uint256 chainId,address verifyingContract)")),
            "8b73c3c69bb8fe3d512ecc4cf759cc79239f7b179b0ffacaa9a75d522b39400f"
        );
        let d = relay_calldata(
            &[1; 20], &[2; 20], 1, 0, 2, &[3; 32], 27, &[4; 32], &[5; 32],
        );
        assert_eq!(hex::encode(&d[..4]), "e3ee160e");
        assert_eq!(d.len(), 4 + 9 * 32);
    }

    #[test]
    fn addresses_parse_and_normalise() {
        assert_eq!(
            norm_address("0x036CbD53842c5426634e7929541eC2318f3dCF7e").as_deref(),
            Some(BASE_USDC)
        );
        for bad in [
            "",
            "0x",
            "036cbd53842c5426634e7929541ec2318f3dcf7e",
            "0x036cbd53842c5426634e7929541ec2318f3dcf7",
            "0xzz6cbd53842c5426634e7929541ec2318f3dcf7e",
        ] {
            assert_eq!(norm_address(bad), None, "{bad}");
        }
    }

    #[test]
    fn nonces_differ_per_agent_and_attempt() {
        assert_ne!(auth_nonce("a.testnet", 0), auth_nonce("a.testnet", 1));
        assert_ne!(auth_nonce("a.testnet", 0), auth_nonce("b.testnet", 0));
    }
}
