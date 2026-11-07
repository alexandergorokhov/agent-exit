//! Agent Exit contract, deployed on the agent's own account (prototype A,
//! "locked account": after setup the account keeps no full-access key; the
//! owner acts from their own account, the runner holds one function-call key
//! that this contract added).
//!
//! One contract holds:
//! - the deed: a one-token NEP-171 NFT (`token_id = "deed"`); whoever holds it
//!   is the owner;
//! - the earnings meter: payments arrive as `ft_transfer_call` and are counted
//!   by payer and day, with `self` / `seller_subaccount` / `round_trip` flags;
//! - the listing (`listing_version`, price, earn-out terms, code hash);
//! - `buy()`: an `ft_transfer_call` with `msg = {"kind":"buy",...}` that runs
//!   a self key batch (delete the seller's runner keys, add the buyer's runner
//!   key) and pays out only in the callback, all-or-nothing;
//! - the earn-out escrow (30% of the price), released from counted post-sale
//!   revenue; silence pays the floor; a proven backdoor cuts it;
//! - the spending policy (feature 12): the owner sets limits with
//!   `set_limits`, every incoming payment is split between the owner's revenue
//!   and the agent's treasury, and the runner key can `pay` out of the
//!   treasury only to allowed payees and only up to the daily cap.
//!
//! All rules live in `agent-core`; this file wires them to the NEAR runtime.

use agent_core::{
    self as core, Amount, BuyResult, Limits, Machine, PayResult, RunnerCheck, Terms, TransferKind,
};
use near_contract_standards::fungible_token::core::ext_ft_core;
use near_contract_standards::non_fungible_token::approval::ext_nft_approval_receiver;
use near_contract_standards::non_fungible_token::core::ext_nft_receiver;
use near_contract_standards::non_fungible_token::events::{NftMint, NftTransfer};
use near_contract_standards::non_fungible_token::metadata::{
    NFTContractMetadata, TokenMetadata, NFT_METADATA_SPEC,
};
use near_contract_standards::non_fungible_token::{Token, TokenId};
use near_sdk::json_types::{U128, U64};
use near_sdk::serde::{Deserialize, Serialize};
use near_sdk::serde_json::{self, json, Value};
use near_sdk::NearSchema;
use near_sdk::{
    env, near, require, AccountId, Allowance, Gas, NearToken, PanicOnDefault, Promise,
    PromiseError, PromiseOrValue, PublicKey,
};
use std::collections::{BTreeSet, HashMap};

#[cfg(test)]
mod base_tests;
#[cfg(test)]
mod fuzz_tests;
pub mod meter_store;
#[cfg(test)]
mod review2_regressions;
#[cfg(test)]
mod review3_regressions;
#[cfg(test)]
mod review4_regressions;
#[cfg(test)]
mod review_regressions;
#[cfg(test)]
mod shares_tests;
#[cfg(any(test, feature = "sim"))]
pub mod sim;
#[cfg(test)]
mod tests;
pub mod verified;

/// C27: at most this many NEAR AI signing addresses (each costs an ecrecover per verified payment).
pub const MAX_SIGNERS: usize = 8;
pub const DEED_ID: &str = "deed";

/// Registry admin (edits the NEAR AI signer list only) and the one accepted
/// payment token. They are compiled into the wasm, not chosen at init, so the
/// published code hash pins them (A26). Set at build time with
/// `AGENT_EXIT_ADMIN` / `AGENT_EXIT_TOKEN`; the defaults are the offline
/// fixture accounts.
pub const REGISTRY_ADMIN: &str = match option_env!("AGENT_EXIT_ADMIN") {
    Some(a) => a,
    None => "admin.testnet",
};
/// Protocol fee account (1% of each sale and share buy, claimable; see
/// `agent_core::money::PROTOCOL_FEE_BPS`). Defaults to the registry admin.
pub const FEE_ACCOUNT: &str = match option_env!("AGENT_EXIT_FEE_ACCOUNT") {
    Some(a) => a,
    None => REGISTRY_ADMIN,
};
pub const PAYMENT_TOKEN: &str = match option_env!("AGENT_EXIT_TOKEN") {
    Some(t) => t,
    None => "xusd.testnet",
};
/// Shortest earn-out window a listing may have, in seconds (A32). The
/// published (prod) build uses 30 days. A demo build may shorten it with
/// `AGENT_EXIT_MIN_WINDOW_SECS`; `get_status` and every listing then say
/// `"terms_config": "demo"`.
pub const MIN_WINDOW_SECS: u64 = match option_env!("AGENT_EXIT_MIN_WINDOW_SECS") {
    Some(v) => parse_u64(v),
    None => core::TermsMin::PROD.min_window_secs,
};

/// The minimum terms this build enforces (A32).
pub const TERMS_MIN: core::TermsMin = core::TermsMin {
    min_window_secs: MIN_WINDOW_SECS,
    ..core::TermsMin::PROD
};

const fn parse_u64(s: &str) -> u64 {
    let b = s.as_bytes();
    assert!(!b.is_empty(), "AGENT_EXIT_MIN_WINDOW_SECS is empty");
    let mut i = 0;
    let mut n: u64 = 0;
    while i < b.len() {
        assert!(
            b[i].is_ascii_digit(),
            "AGENT_EXIT_MIN_WINDOW_SECS is not a number"
        );
        n = n * 10 + (b[i] - b'0') as u64;
        i += 1;
    }
    assert!(n >= 60, "AGENT_EXIT_MIN_WINDOW_SECS must be at least 60");
    n
}

/// The Chain Signatures MPC signer the Base wallet uses (feature 5). Pinned
/// at build time like the token; testnet default.
pub const MPC_SIGNER: &str = match option_env!("AGENT_EXIT_MPC") {
    Some(a) => a,
    None => "v1.signer-prod.testnet",
};
/// MPC `sign` domain 0 = secp256k1 (Ethereum-style keys).
pub const MPC_DOMAIN_SECP256K1: u64 = 0;
/// Gas a runner attaches to `pay_base`: the MPC `sign` call gets what is
/// left after the callback's share.
pub const RUNNER_GAS_PAY_BASE: Gas = Gas::from_tgas(300);
pub const GAS_MPC_SIGN_MIN: Gas = Gas::from_tgas(100);
pub const GAS_ON_BASE_SIGNED: Gas = Gas::from_tgas(20);

/// Methods the runner's function-call key may call (receiver = the agent).
pub const RUNNER_METHODS: &str = "heartbeat,accept_job,pay,pay_base";
/// Gas budget of the runner's function-call key. The node checks it against
/// the attached gas at a pessimistic gas price before every call, so the
/// runner attaches only what a call needs (`RUNNER_GAS_*`). 5 NEAR covers
/// thousands of heartbeats; the owner refreshes it by rotating the key.
pub const RUNNER_ALLOWANCE: NearToken = NearToken::from_near(5);
/// Gas a runner attaches: `heartbeat` (state read and write only).
pub const RUNNER_GAS_HEARTBEAT: Gas = Gas::from_tgas(30);
/// Gas a runner attaches: `pay` (ft_transfer 10 TGas + callback 15 TGas,
/// both plus the state term, + execution). 100 TGas covers the largest
/// state the bounded tables allow (A43).
pub const RUNNER_GAS_PAY: Gas = Gas::from_tgas(100);
/// NEAR kept on the account on top of storage staking (E17).
pub const NEAR_RESERVE: NearToken = NearToken::from_near(1);
/// Upper bound of the storage one used signature id takes: its `expires`
/// entry (key 34 + value 8) and ring slot (key 6 + value 32), each with the
/// 40-byte record overhead, rounded up (A46).
pub const SIG_ENTRY_BYTES: u128 = 200;
/// Upper bound of one payer row: key (2 + 64) + value (75) + 40 (A47).
pub const PAYER_ENTRY_BYTES: u128 = 200;
/// Upper bound of a row's deposit record (who paid, when, how much):
/// key (2 + 64) + value (4 + 64 + 8 + 16) + 40 (A50).
pub const PAYER_RECORD_BYTES: u128 = 200;
/// Upper bound of one of a payer's own signature slots: the slot (key
/// 2 + 64 + 1, value 40) and its `expires` entry (key 34, value 8), each
/// with the 40-byte overhead (A54).
pub const PAYER_SIG_SLOT_BYTES: u128 = 250;
/// Storage a registered payer may take: row, record and signature slots.
pub const PAYER_BYTES: u128 =
    PAYER_ENTRY_BYTES + PAYER_RECORD_BYTES + verified::PAYER_SIGS as u128 * PAYER_SIG_SLOT_BYTES;
/// NEAR a payer row costs (`storage_deposit`, A49): its storage at 1e19
/// yoctoNEAR per byte. Refunded when an empty row is removed (A50).
pub const PAYER_ROW_DEPOSIT: NearToken =
    NearToken::from_yoctonear(PAYER_BYTES * 10_000_000_000_000_000_000);
/// A row with no payment this long after its deposit may be reclaimed by
/// anyone; the deposit goes back to whoever paid it (A50).
pub const EMPTY_ROW_TTL_NS: u64 = 30 * core::DAY_NS;
/// Storage prefix of the deposit records.
const DEPOSIT_PREFIX: &[u8] = b"rd";
/// Upper bound of one day row and its list slot (A47).
pub const DAY_ENTRY_BYTES: u128 = 200;
/// Longest `ft_transfer_call` msg accepted (E10).
pub const MAX_MSG_LEN: usize = 4_096;

pub const GAS_FT_TRANSFER: Gas = Gas::from_tgas(10);
/// Callback gas before the state term (`cb_gas`).
pub const GAS_ON_TRANSFER_OUT: Gas = Gas::from_tgas(15);
pub const GAS_ON_HANDOVER: Gas = Gas::from_tgas(40);
pub const GAS_ON_KEYS: Gas = Gas::from_tgas(15);
/// Gas per byte of contract storage a callback may read and write: storage
/// read (~5.6 Mgas/byte) plus write (~63 Mgas/byte), rounded up (A33).
pub const GAS_PER_STATE_BYTE: u64 = 70_000_000;
pub const GAS_NFT_ON_TRANSFER: Gas = Gas::from_tgas(30);
pub const GAS_NFT_RESOLVE: Gas = Gas::from_tgas(20);
pub const GAS_NFT_ON_APPROVE: Gas = Gas::from_tgas(20);
/// Gas that must be left when a buy starts: key batch + callback + payout (E7).
pub const MIN_BUY_GAS: Gas = Gas::from_tgas(70);

/// `ft_transfer_call` msg. Empty msg = a plain payment.
#[derive(Deserialize, Debug)]
#[serde(crate = "near_sdk::serde", tag = "kind", rename_all = "snake_case")]
pub enum Msg {
    Pay {
        #[serde(default)]
        job_id: Option<String>,
        /// NEAR AI Cloud signature of the answer this payment is for.
        #[serde(default)]
        nearai: Option<verified::NearAiProof>,
    },
    Buy {
        listing_version: u64,
        known_keys_hash: String,
        buyer_runner_pk: PublicKey,
        /// One-time X25519 key (hex) for a listing with a sealed config.
        #[serde(default)]
        delivery_pk: Option<String>,
    },
    /// Buy `part` bps of a share slice, with the terms the investor saw (T6).
    BuyShare {
        offer_id: u64,
        bps: u16,
        price: U128,
        end: U64,
        part: u16,
    },
}

#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq, NearSchema)]
#[serde(crate = "near_sdk::serde", rename_all = "snake_case")]
#[abi(json)]
pub enum OutKind {
    SellerPayout,
    Claim,
    OwnerWithdraw,
    TreasuryPay,
}

/// Revenue split of every incoming payment; must add up to 10,000 bps.
#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq, NearSchema)]
#[serde(crate = "near_sdk::serde", deny_unknown_fields)]
#[abi(json)]
pub struct Split {
    pub owner_bps: u16,
    pub treasury_bps: u16,
}

/// `set_limits` arguments (`limits.v1`). Amounts are token base units as
/// strings; a negative or non-numeric amount fails to parse (E33).
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq, NearSchema)]
#[serde(crate = "near_sdk::serde", deny_unknown_fields)]
#[abi(json)]
pub struct LimitsArgs {
    /// Smallest accepted incoming payment.
    pub price_floor: U128,
    /// Most the runner may pay per UTC day, counted per owner.
    pub daily_cap: U128,
    /// The only accounts the runner may pay.
    pub allowed_payees: Vec<AccountId>,
    pub split: Split,
    /// Base Sepolia limits (feature 5); absent = keep the current ones
    /// (A52). `{"daily_cap": "0", "payees": []}` clears them.
    #[serde(default)]
    pub base: Option<BaseLimitsArgs>,
}

/// Base Sepolia limits inside `set_limits` (feature 5). Leaving `base` out
/// keeps the current Base limits (A52); a 0 cap and no payees clears them.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq, NearSchema)]
#[serde(crate = "near_sdk::serde", deny_unknown_fields)]
#[abi(json)]
pub struct BaseLimitsArgs {
    /// Most USDC (6 decimals, base units) the runner may authorize per UTC day.
    pub daily_cap: U128,
    /// The only Base addresses the runner may pay (`0x` + 40 hex).
    pub payees: Vec<String>,
}

impl From<BaseLimitsArgs> for core::base::BaseLimits {
    fn from(a: BaseLimitsArgs) -> Self {
        core::base::BaseLimits {
            daily_cap: a.daily_cap.0,
            payees: a
                .payees
                .iter()
                .map(|p| core::base::norm_address(p).unwrap_or_else(|| p.clone()))
                .collect(),
        }
    }
}

impl From<LimitsArgs> for Limits {
    fn from(a: LimitsArgs) -> Self {
        Limits {
            price_floor: a.price_floor.0,
            daily_cap: a.daily_cap.0,
            allowed_payees: a.allowed_payees.iter().map(|p| p.to_string()).collect(),
            owner_bps: a.split.owner_bps,
            treasury_bps: a.split.treasury_bps,
        }
    }
}

impl From<OutKind> for TransferKind {
    fn from(k: OutKind) -> Self {
        match k {
            OutKind::SellerPayout => TransferKind::SellerPayout,
            OutKind::Claim => TransferKind::Claim,
            OutKind::OwnerWithdraw => TransferKind::OwnerWithdraw,
            OutKind::TreasuryPay => TransferKind::TreasuryPay,
        }
    }
}

#[near(contract_state)]
#[derive(PanicOnDefault)]
pub struct AgentExit {
    m: Machine<meter_store::NearStore>,
    /// The one accepted NEP-141 token (the FAKE test dollar in the demo).
    token: AccountId,
    /// Registry admin: may only edit the NEAR AI signer list (threat model
    /// AC10); no power over funds, keys or the deed.
    admin: AccountId,
    /// Allowed NEAR AI Cloud signing addresses (`0x` + 40 lowercase hex).
    signers: BTreeSet<String>,
    /// sha256(r || s) of every signature that already verified a payment.
    /// One storage entry per signature, outside the state blob that every
    /// call reads and writes (A33).
    pub used_sigs: verified::UsedSigs,
    /// NEAR held for payer rows: the sum of their deposits, refundable when
    /// an empty row is removed and prepaying the payers' signature slots.
    /// The owner cannot withdraw it (A50).
    pub payer_deposits: u128,
}

fn emit(event: &str, data: Value) {
    env::log_str(&format!(
        "EVENT_JSON:{}",
        json!({"standard": "agent_exit", "version": "1.0.0", "event": event, "data": [data]})
    ));
}

fn sealed_key(sale_id: u64) -> Vec<u8> {
    [b"sc:".as_slice(), &sale_id.to_le_bytes()].concat()
}

fn fail(e: core::Error) -> ! {
    env::panic_str(&e.to_string())
}

/// Did promise 0 succeed? A long result still means success.
fn promise_ok() -> bool {
    !matches!(
        env::promise_result_checked(0, 1_024),
        Err(PromiseError::Failed)
    )
}

fn pk(s: &str) -> PublicKey {
    s.parse().unwrap_or_else(|_| env::panic_str("BadKey"))
}

fn amt(a: Amount) -> String {
    a.to_string()
}

/// `53000000` -> `53.00` (6-decimal test dollar), for error messages.
fn usd(a: Amount) -> String {
    format!("{}.{:02}", a / core::USD, (a % core::USD) / 10_000)
}

#[near]
impl AgentExit {
    /// Called once by the agent account itself at deploy (same tx as deploy).
    /// The deployer (the seller) chooses only the first owner: the admin, the
    /// token and the signer list are not init arguments (A26). The signer
    /// list starts empty; only the pinned registry admin edits it.
    ///
    /// `base_address`: the agent's Chain Signatures Base address (derived
    /// from this account and path `wallet-1`; `tools/base-wallet derive`).
    /// It cannot change later. The contract cannot derive it (the MPC public
    /// key is not available on chain at init), so this is a free argument:
    /// every MPC signature is checked against it, which makes `pay_base`
    /// fail with a wrong one, and the buyer's scan derives the address from
    /// the MPC public key and blocks a mismatch (`base_address_mismatch`,
    /// A51). Nothing else ties it to the derivation.
    #[init]
    #[private]
    pub fn new(owner_id: AccountId, base_address: Option<String>) -> Self {
        let mut m = Machine::<meter_store::NearStore>::with_store(
            env::current_account_id().as_str(),
            owner_id.as_str(),
        );
        m.terms_min = TERMS_MIN;
        m.fee_to = FEE_ACCOUNT.into();
        m.base.address = base_address.map(|a| norm_address(&a));
        // NEP-171: the deed exists from init; indexers see its mint.
        NftMint {
            owner_id: &owner_id,
            token_ids: &[DEED_ID],
            memo: Some("agent_exit:deed"),
        }
        .emit();
        Self {
            m,
            token: PAYMENT_TOKEN.parse().expect("valid token"),
            admin: REGISTRY_ADMIN.parse().expect("valid admin"),
            signers: BTreeSet::new(),
            used_sigs: verified::UsedSigs::new(b'u'),
            payer_deposits: 0,
        }
    }

    // ------------------------------------------------------- admin: signers

    /// Admin adds a NEAR AI Cloud signing address. Payments verified before
    /// keep their badge whatever happens to the list later (E14).
    #[payable]
    pub fn add_signer(&mut self, address: String) {
        self.only_admin();
        // A function-call key of the admin cannot change who signs "Verified".
        near_sdk::assert_one_yocto();
        let a = norm_address(&address);
        // C27: each signer costs an ecrecover in every verified payment.
        require!(
            self.signers.contains(&a) || self.signers.len() < MAX_SIGNERS,
            "TooManySigners"
        );
        self.signers.insert(a.clone());
        emit("signer_added", json!({"address": a}));
    }

    #[payable]
    pub fn remove_signer(&mut self, address: String) {
        self.only_admin();
        // A function-call key of the admin cannot change who signs "Verified".
        near_sdk::assert_one_yocto();
        let a = norm_address(&address);
        self.signers.remove(&a);
        emit("signer_removed", json!({"address": a}));
    }

    pub fn get_signers(&self) -> Vec<String> {
        self.signers.iter().cloned().collect()
    }

    // ------------------------------------------------------------ owner: keys

    /// Owner sets (or rotates) the runner's function-call key. The contract
    /// deletes every runner key it added before and adds this one, scoped to
    /// this account and `RUNNER_METHODS`, with no deposit allowed.
    #[payable]
    pub fn set_runner_key(&mut self, public_key: PublicKey) -> Promise {
        near_sdk::assert_one_yocto();
        let caller = env::predecessor_account_id();
        let new_key = String::from(&public_key);
        let old: Vec<String> = self.m.known_keys.iter().cloned().collect();
        let deleted = self
            .m
            .set_runner_key(caller.as_str(), &new_key, env::block_timestamp())
            .unwrap_or_else(|e| fail(e));
        let mut p = Promise::new(env::current_account_id());
        for k in &deleted {
            p = p.delete_key(pk(k));
        }
        p = p.add_access_key_allowance(
            public_key,
            Allowance::limited(RUNNER_ALLOWANCE).expect("non-zero"),
            env::current_account_id(),
            RUNNER_METHODS,
        );
        emit(
            "runner_key_set",
            json!({"owner": caller, "key": new_key, "removed": deleted, "pending": true}),
        );
        p.then(
            Self::ext(env::current_account_id())
                .with_static_gas(self.cb_gas(GAS_ON_KEYS))
                .on_runner_key(old, deleted, new_key, self.m.key_batch),
        )
    }

    /// Callback of the rotation batch (all-or-nothing): success makes the new
    /// key the runner key, failure restores the old one (A25). `batch` is
    /// the batch's nonce; a callback that lands after `recover_keys` is
    /// stale and changes no key set. If its batch added a key no set names
    /// any more, that key is deleted (A44).
    #[private]
    pub fn on_runner_key(
        &mut self,
        old: Vec<String>,
        deleted: Vec<String>,
        new_key: String,
        batch: u64,
    ) -> bool {
        let ok = promise_ok();
        if !self.m.runner_key_done(batch, &old, &deleted, &new_key, ok) {
            let delete = if ok {
                self.m.retire_untracked(&new_key)
            } else {
                None
            };
            emit(
                "stale_callback",
                json!({"callback": "on_runner_key", "batch": batch, "key_batch": self.m.key_batch,
                       "key": new_key, "batch_ok": ok, "delete_keys": delete}),
            );
            if let Some(k) = delete {
                self.delete_keys(&[k]);
            }
            return false;
        }
        emit(
            if ok {
                "runner_key_confirmed"
            } else {
                "runner_key_failed"
            },
            json!({"key": new_key, "deleted": if ok { deleted } else { vec![] }}),
        );
        ok
    }

    /// Callback after `delete_key`s, one receipt per key (deed transfer,
    /// recovery). Each has run, so none of the keys is on the account any
    /// more: a failed `delete_key` means the key was already gone (A25, A33).
    #[private]
    pub fn on_keys_deleted(&mut self, keys: Vec<String>) -> bool {
        let n = env::promise_results_count();
        let already_gone = (0..n)
            .filter(|i| {
                matches!(
                    env::promise_result_checked(*i, 1_024),
                    Err(PromiseError::Failed)
                )
            })
            .count();
        self.m.keys_deleted(&keys);
        emit(
            "keys_deleted",
            json!({"keys": keys, "already_gone": already_gone}),
        );
        true
    }

    // ------------------------------------------------- recovery (A33)

    /// The owner clears keys whose key-batch callback failed (they stay
    /// `incoming`/`retiring` and block listing and rotation). Allowed an hour
    /// after the batch started. Deletes each pending key in its own receipt;
    /// runner keys the contract confirmed stay. Needs 1 yoctoNEAR.
    #[payable]
    pub fn recover_keys(&mut self) -> Value {
        near_sdk::assert_one_yocto();
        let caller = env::predecessor_account_id();
        let r = self
            .m
            .recover_keys(caller.as_str(), env::block_timestamp())
            .unwrap_or_else(|e| fail(e));
        self.delete_keys(&r.delete_keys);
        let data = json!({"by": caller, "delete_keys": r.delete_keys, "paused": self.m.paused});
        emit("keys_recovered", data.clone());
        data
    }

    /// Anyone undoes a buy whose `on_handover` callback failed (the agent
    /// would stay `Settling` for good). Allowed an hour after the buy. The
    /// failed callback already made the token refund the buyer in full; the
    /// seller keeps the deed, the listing is withdrawn and every key the buy
    /// touched is deleted. The seller sets a new runner key to resume.
    pub fn recover_sale(&mut self) -> Value {
        let r = self
            .m
            .recover_sale(env::block_timestamp())
            .unwrap_or_else(|e| fail(e));
        self.delete_keys(&r.delete_keys);
        let p = r.sale.expect("recover_sale returns the sale");
        let data = json!({
            "sale_id": p.sale_id, "buyer": p.buyer, "amount": amt(p.amount),
            "buyer_refunded_by_token": true, "owner": self.m.owner,
            "delete_keys": r.delete_keys, "listing_version": self.m.listing_version,
        });
        emit("sale_recovered", data.clone());
        data
    }

    // --------------------------------------------------------- owner: listing

    /// Needs 1 yoctoNEAR: only a full-access key of the owner can list (A28).
    #[payable]
    pub fn list_for_sale(&mut self, price: U128, code_hash: String, terms: Option<Terms>) -> Value {
        near_sdk::assert_one_yocto();
        let caller = env::predecessor_account_id();
        let l = self
            .m
            .list(
                caller.as_str(),
                price.0,
                terms.unwrap_or_default(),
                &code_hash,
                env::block_timestamp(),
            )
            .unwrap_or_else(|e| fail(e));
        let card = self.listing_json(&l);
        emit("listed", card.clone());
        card
    }

    #[payable]
    pub fn cancel_listing(&mut self) {
        near_sdk::assert_one_yocto();
        let caller = env::predecessor_account_id();
        self.m
            .cancel_listing(caller.as_str())
            .unwrap_or_else(|e| fail(e));
        emit(
            "listing_cancelled",
            json!({"owner": caller, "listing_version": self.m.listing_version}),
        );
    }

    // ------------------------------------------------- NEP-141 receiver: money

    /// Every incoming payment and every buy arrives here from the token.
    /// Any refusal returns the full amount as unused, so the token refunds the
    /// sender in `ft_resolve_transfer`. A call from any other account than the
    /// accepted token panics `UnknownToken` (E8, E9, A14).
    pub fn ft_on_transfer(
        &mut self,
        sender_id: AccountId,
        amount: U128,
        msg: String,
    ) -> PromiseOrValue<U128> {
        if env::predecessor_account_id() != self.token {
            env::panic_str("UnknownToken");
        }
        let refuse = |reason: &str| {
            emit(
                "refused",
                json!({"sender": sender_id, "amount": amt(amount.0), "reason": reason}),
            );
            PromiseOrValue::Value(amount)
        };
        if msg.len() > MAX_MSG_LEN {
            return refuse("BadMsg");
        }
        let parsed = if msg.trim().is_empty() {
            Msg::Pay {
                job_id: None,
                nearai: None,
            }
        } else {
            match serde_json::from_str::<Msg>(&msg) {
                Ok(m) => m,
                Err(_) => return refuse("BadMsg"),
            }
        };
        let now = env::block_timestamp();
        match parsed {
            Msg::Pay { job_id, nearai } => {
                // A54: a registered payer's proofs use its own slots first.
                let registered = !self.m.store_payer_missing(sender_id.as_str());
                let check = nearai.as_ref().map(|p| {
                    verified::check(
                        p,
                        self.m.agent.as_str(),
                        sender_id.as_str(),
                        amount.0,
                        job_id.as_deref(),
                        &self.signers,
                        &self.used_sigs,
                        registered,
                        now / core::NANOS_PER_SEC,
                    )
                });
                let proof_ok = matches!(check, Some(verified::Outcome::Verified { .. }));
                let verify_note = match &check {
                    None => "NoProof".to_string(),
                    Some(verified::Outcome::Verified { signer, .. }) => format!("ok:{signer}"),
                    Some(verified::Outcome::NotVerified(r)) => r.to_string(),
                };
                match self.m.pay(sender_id.as_str(), amount.0, proof_ok, now) {
                    PayResult::Refund(r) => refuse(r),
                    PayResult::Recorded {
                        flags,
                        counted,
                        verified,
                        released,
                        to_owner,
                        to_treasury,
                        to_investors,
                        share_released,
                    } => {
                        if let (
                            true,
                            Some(verified::Outcome::Verified {
                                sig_id, expires, ..
                            }),
                        ) = (verified, &check)
                        {
                            self.used_sigs.insert_for(
                                registered.then_some(sender_id.as_str()),
                                *sig_id,
                                *expires,
                                now / core::NANOS_PER_SEC,
                            );
                        }
                        emit(
                            "payment",
                            json!({
                                "payer": sender_id, "amount": amt(amount.0), "job_id": job_id,
                                "counted": counted, "verified": verified, "verify": verify_note,
                                "flags": flags,
                                "day": now / core::DAY_NS,
                            }),
                        );
                        // The automatic split. While an earn-out is open, the
                        // seller's share of this payment is the earn-out release
                        // (paid from escrow, not from the payment).
                        let earnout: Amount = released.iter().map(|r| r.2).sum();
                        emit(
                            "split",
                            json!({
                                "amount": amt(amount.0), "to_owner": amt(to_owner),
                                "to_treasury": amt(to_treasury),
                                "to_investors": amt(to_investors),
                                "seller_earnout_release": amt(earnout),
                            }),
                        );
                        if to_investors > 0 {
                            emit(
                                "share_paid",
                                json!({"amount": amt(to_investors),
                                       "escrow_released": share_released.iter()
                                           .map(|(s, r)| json!({"seller": s, "amount": amt(*r)}))
                                           .collect::<Vec<_>>()}),
                            );
                        }
                        for (sale_id, seller, r) in released {
                            emit(
                                "earnout_release",
                                json!({"sale_id": sale_id, "seller": seller, "amount": amt(r)}),
                            );
                        }
                        PromiseOrValue::Value(U128(0))
                    }
                }
            }
            Msg::BuyShare {
                offer_id,
                bps,
                price,
                end,
                part,
            } => {
                let seen = core::ShareTerms {
                    offer_id,
                    bps,
                    price: price.0,
                    end: end.0,
                };
                match self
                    .m
                    .buy_share(sender_id.as_str(), amount.0, seen, part, now)
                {
                    Ok(cost) => {
                        emit(
                            "share_bought",
                            json!({"offer_id": offer_id, "investor": sender_id, "part_bps": part,
                                   "cost": amt(cost),
                                   "protocol_fee": amt(if self.m.fee_to.is_empty() { 0 } else { core::money::protocol_fee(cost).min(cost - cost / 2) }),
                                   "fee_to": self.m.fee_to}),
                        );
                        PromiseOrValue::Value(U128(amount.0 - cost))
                    }
                    Err(e) => refuse(&e.to_string()),
                }
            }
            Msg::Buy {
                listing_version,
                known_keys_hash,
                buyer_runner_pk,
                delivery_pk,
            } => {
                let left = env::prepaid_gas().saturating_sub(env::used_gas());
                // The callback and the payout callback each read and write
                // the state, so the bar grows with it (A33).
                let gas_ok = left
                    >= MIN_BUY_GAS
                        .saturating_add(self.state_gas())
                        .saturating_add(self.state_gas());
                let buyer_key = String::from(&buyer_runner_pk);
                let plan = match self.m.begin_buy_sealed(
                    sender_id.as_str(),
                    amount.0,
                    listing_version,
                    &known_keys_hash,
                    &buyer_key,
                    delivery_pk.as_deref(),
                    gas_ok,
                    now,
                ) {
                    Ok(p) => p,
                    Err(e) => return refuse(&e.to_string()),
                };
                emit(
                    "settling",
                    json!({"sale_id": plan.sale_id, "buyer": sender_id, "amount": amt(amount.0),
                           "delete_keys": plan.delete_keys, "add_key": plan.add_key}),
                );
                // One receipt on this account: all key actions apply or none do.
                let mut batch = Promise::new(env::current_account_id());
                for k in &plan.delete_keys {
                    batch = batch.delete_key(pk(k));
                }
                batch = batch.add_access_key_allowance(
                    buyer_runner_pk,
                    Allowance::limited(RUNNER_ALLOWANCE).expect("non-zero"),
                    env::current_account_id(),
                    RUNNER_METHODS,
                );
                batch
                    .then(
                        Self::ext(env::current_account_id())
                            .with_static_gas(self.cb_gas(GAS_ON_HANDOVER))
                            .on_handover(plan.sale_id),
                    )
                    .into()
            }
        }
    }

    /// Callback after the key batch. Returns the unused amount to the token:
    /// the overpay on success, the whole payment on failure (rollback).
    #[private]
    pub fn on_handover(&mut self, sale_id: u64) -> U128 {
        let ok = promise_ok();
        let now = env::block_timestamp();
        // A44: the callback of a buy `recover_sale` already undid.
        if let Some((refund, delete)) = self.m.stale_handover(sale_id, ok) {
            emit(
                "stale_callback",
                json!({"callback": "on_handover", "sale_id": sale_id, "batch_ok": ok,
                       "refund": amt(refund), "delete_keys": delete}),
            );
            if let Some(k) = delete {
                self.delete_keys(&[k]);
            }
            return U128(refund);
        }
        let res = self
            .m
            .finish_buy(sale_id, ok, now)
            .unwrap_or_else(|e| fail(e));
        match res {
            BuyResult::RolledBack {
                unused,
                frozen,
                delete_keys,
            } => {
                // A34: frozen while settling = the buy is undone too.
                self.delete_keys(&delete_keys);
                emit(
                    "sale_rolled_back",
                    json!({"sale_id": sale_id, "refund": amt(unused), "frozen": frozen,
                           "delete_keys": delete_keys}),
                );
                U128(unused)
            }
            BuyResult::Done {
                sale,
                seller,
                payout,
                fee,
                unused,
                seller_revenue,
            } => {
                let seller_id: AccountId = seller.parse().expect("valid");
                let buyer_id: AccountId = sale.buyer.parse().expect("valid");
                NftTransfer {
                    old_owner_id: &seller_id,
                    new_owner_id: &buyer_id,
                    token_ids: &[DEED_ID],
                    authorized_id: None,
                    memo: Some("agent_exit:buy"),
                }
                .emit();
                emit(
                    "sale",
                    json!({
                        "sale_id": sale.id, "seller": seller, "buyer": sale.buyer,
                        "price": amt(sale.price), "paid_now": amt(payout),
                        "protocol_fee": amt(fee), "fee_to": self.m.fee_to,
                        "escrow": amt(sale.escrow), "overpay_refund": amt(unused),
                        "seller_revenue_claimable": amt(seller_revenue),
                        "owner_epoch": self.m.owner_epoch,
                        "window_end": U64(sale.window_end),
                        "known_keys": self.m.known_keys,
                    }),
                );
                // A pay-from-revenue sale has no upfront payout (PR1); a
                // zero ft_transfer would only fail at the token.
                if payout > 0 {
                    self.send(OutKind::SellerPayout, &seller, payout).detach();
                }
                U128(unused)
            }
        }
    }

    /// Push falls back to pull: a failed transfer is credited back (E6).
    #[private]
    pub fn on_transfer_out(
        &mut self,
        kind: OutKind,
        to: AccountId,
        amount: U128,
        by: Option<AccountId>,
    ) -> bool {
        let ok = promise_ok();
        self.m.transfer_done(
            kind.into(),
            to.as_str(),
            by.as_ref().map(|b| b.as_str()),
            amount.0,
            ok,
        );
        if !ok {
            emit(
                "transfer_failed",
                json!({"kind": kind, "to": to, "amount": amt(amount.0), "credited_back": true}),
            );
        }
        ok
    }

    /// Pull a claimable balance (failed payouts, earn-out releases, refunds).
    pub fn claim(&mut self) -> Promise {
        let caller = env::predecessor_account_id();
        let a = self.m.claim(caller.as_str()).unwrap_or_else(|e| fail(e));
        self.send(OutKind::Claim, caller.as_str(), a)
    }

    /// Owner takes revenue out, to themselves or `to`. Needs 1 yoctoNEAR so
    /// only a full-access key of the owner can call it.
    #[payable]
    pub fn withdraw(&mut self, amount: U128, to: Option<AccountId>) -> Promise {
        near_sdk::assert_one_yocto();
        let caller = env::predecessor_account_id();
        let to = to.unwrap_or_else(|| caller.clone());
        let a = self
            .m
            .withdraw(caller.as_str(), amount.0, to.as_str())
            .unwrap_or_else(|e| fail(e));
        self.send_by(OutKind::OwnerWithdraw, to.as_str(), a, Some(caller))
    }

    /// Owner takes NEAR above storage staking plus `NEAR_RESERVE` (E17, I10).
    #[payable]
    pub fn withdraw_near(&mut self, amount: NearToken) -> Promise {
        near_sdk::assert_one_yocto();
        let caller = env::predecessor_account_id();
        if caller.as_str() != self.m.owner {
            fail(core::Error::NotOwner);
        }
        if self.m.status == core::Status::Settling {
            fail(core::Error::SaleInProgress);
        }
        // A39: the buyer scanned the agent's NEAR balance too.
        if self.m.status == core::Status::Listed {
            fail(core::Error::SpendingPaused);
        }
        if amount > self.near_withdrawable() {
            env::panic_str("StorageReserve");
        }
        Promise::new(caller).transfer(amount)
    }

    // ------------------------------- sealed agent config (SC1-SC12, sealed.rs)

    /// Owner commits to the sealed config (system prompt, tool setup, the API
    /// keys a buyer must rotate) the next listing sells: hex sha256 of the
    /// bundle and its length. Empty `config_hash` clears it. 1 yoctoNEAR.
    #[payable]
    pub fn commit_config(&mut self, config_hash: String, bytes: u32) -> Value {
        near_sdk::assert_one_yocto();
        let caller = env::predecessor_account_id();
        let commit = (!config_hash.is_empty()).then_some(core::sealed::ConfigCommit {
            hash: config_hash,
            bytes,
        });
        self.m
            .commit_config(caller.as_str(), commit.clone())
            .unwrap_or_else(|e| fail(e));
        emit(
            "config_committed",
            json!({"owner": caller, "config": commit}),
        );
        json!({"config": commit})
    }

    /// The seller posts the bundle sealed to the buyer's one-time key
    /// (base64 ciphertext, as long as the bundle). Attach the storage for it
    /// (`bytes + 200` times the storage price); it stays with the agent.
    #[payable]
    pub fn deliver_config(
        &mut self,
        sale_id: u64,
        ephemeral_pk: String,
        ciphertext: near_sdk::json_types::Base64VecU8,
    ) -> Value {
        let caller = env::predecessor_account_id();
        let cipher: Vec<u8> = ciphertext.into();
        let need = env::storage_byte_cost().saturating_mul(cipher.len() as u128 + 200);
        require!(
            env::attached_deposit() >= need,
            format!("attach {need} yoctoNEAR for the ciphertext's storage")
        );
        self.m
            .deliver_config(
                caller.as_str(),
                sale_id,
                &ephemeral_pk,
                &cipher,
                env::block_timestamp(),
            )
            .unwrap_or_else(|e| fail(e));
        env::storage_write(&sealed_key(sale_id), &cipher);
        emit(
            "config_delivered",
            json!({"sale_id": sale_id, "seller": caller, "ephemeral_pk": ephemeral_pk,
                   "bytes": cipher.len(), "cipher_sha256": core::sealed::to_hex(&env::sha256(&cipher))}),
        );
        json!({"sale_id": sale_id, "delivered": true})
    }

    /// No delivery by the deadline: the remaining escrow is the buyer's
    /// (claimable). Anyone may call.
    pub fn claim_undelivered(&mut self, sale_id: u64) -> Value {
        let r = self
            .m
            .claim_undelivered(sale_id, env::block_timestamp())
            .unwrap_or_else(|e| fail(e));
        emit(
            "config_missed",
            json!({"sale_id": sale_id, "refund_to_buyer": amt(r)}),
        );
        json!({"sale_id": sale_id, "refund_to_buyer": amt(r)})
    }

    /// The buyer reveals the one-time secret key (hex). The contract decrypts
    /// the stored ciphertext; if it is not the committed bundle the
    /// remaining escrow is the buyer's. A matching bundle refuses the dispute.
    pub fn dispute_config(&mut self, sale_id: u64, delivery_sk: String) -> Value {
        let caller = env::predecessor_account_id();
        let cipher = env::storage_read(&sealed_key(sale_id)).unwrap_or_default();
        let r = self
            .m
            .dispute_config(
                caller.as_str(),
                sale_id,
                &delivery_sk,
                &cipher,
                env::block_timestamp(),
            )
            .unwrap_or_else(|e| fail(e));
        emit(
            "config_mismatch",
            json!({"sale_id": sale_id, "buyer": caller, "refund_to_buyer": amt(r)}),
        );
        json!({"sale_id": sale_id, "refund_to_buyer": amt(r)})
    }

    /// A sale's sealed config: the commitment, the buyer's delivery key,
    /// the deadline, the state and the ciphertext (base64) once delivered.
    pub fn get_sealed_config(&self, sale_id: u64) -> Value {
        let s = self
            .m
            .sale(sale_id)
            .unwrap_or_else(|| fail(core::Error::UnknownSale));
        let Some(c) = s.config else {
            return Value::Null;
        };
        let cipher = env::storage_read(&sealed_key(sale_id));
        json!({
            "schema": "sealed-config.v1", "sale_id": sale_id, "commit": c.commit,
            "delivery_pk": c.delivery_pk, "deliver_by": U64(c.deliver_by),
            "dispute_until": U64(c.dispute_until()), "state": c.state,
            "ciphertext": cipher.map(near_sdk::json_types::Base64VecU8::from),
        })
    }

    /// Anyone may close an earn-out after its window (idempotent, E21).
    pub fn finalize_earnout(&mut self, sale_id: u64) -> Value {
        let s = self
            .m
            .finalize_earnout(sale_id, env::block_timestamp())
            .unwrap_or_else(|e| fail(e));
        emit(
            "earnout_finalized",
            json!({"sale_id": s.id, "seller_total": amt(s.released), "buyer_refund": amt(s.returned)}),
        );
        self.sale_json(&s)
    }

    // --------------------------------- shares of future earnings (SH1-SH11)

    /// Owner offers `bps` of every future payment until `end` (ns) for
    /// `price`. Needs 1 yoctoNEAR (a full-access key of the owner).
    #[payable]
    pub fn offer_share(&mut self, bps: u16, price: U128, end: U64) -> Value {
        near_sdk::assert_one_yocto();
        let caller = env::predecessor_account_id();
        let id = self
            .m
            .offer_share(caller.as_str(), bps, price.0, end.0, env::block_timestamp())
            .unwrap_or_else(|e| fail(e));
        let v = self.offer_json(self.m.offer(id).expect("just added"));
        emit("share_offered", v.clone());
        v
    }

    /// Owner withdraws a slice nobody bought (T7).
    #[payable]
    pub fn cancel_offer(&mut self, offer_id: u64) {
        near_sdk::assert_one_yocto();
        let caller = env::predecessor_account_id();
        self.m
            .cancel_offer(caller.as_str(), offer_id)
            .unwrap_or_else(|e| fail(e));
        emit("share_cancelled", json!({"offer_id": offer_id}));
    }

    /// Anyone, after the end: investors get their price escrow back as
    /// claimable balances (SH4, SH11).
    pub fn close_share(&mut self, offer_id: u64) -> Value {
        let refunds = self
            .m
            .close_share(offer_id, env::block_timestamp())
            .unwrap_or_else(|e| fail(e));
        let v = json!({"offer_id": offer_id, "refunds": refunds.iter()
            .map(|(a, r)| json!({"investor": a, "amount": amt(*r)})).collect::<Vec<_>>()});
        emit("share_closed", v.clone());
        v
    }

    /// Open slices with their holders (the scan discloses them, SH7).
    pub fn get_shares(&self) -> Vec<Value> {
        self.m.offers.iter().map(|o| self.offer_json(o)).collect()
    }

    /// One investor's holdings in this agent.
    pub fn get_holdings(&self, account_id: AccountId) -> Value {
        let mut parts = Vec::new();
        for o in &self.m.offers {
            for h in o
                .holders
                .iter()
                .filter(|h| h.account == account_id.as_str())
            {
                parts.push(
                    json!({"offer_id": o.id, "bps": h.bps, "paid_in": amt(h.paid_in),
                    "escrow": amt(h.escrow), "earned": amt(h.earned), "end": U64(o.end)}),
                );
            }
        }
        json!({"agent": self.m.agent, "investor": account_id, "holdings": parts,
               "claimable": amt(self.m.claimable_of(account_id.as_str()))})
    }

    fn offer_json(&self, o: &core::ShareOffer) -> Value {
        json!({
            "offer_id": o.id, "seller": o.seller, "bps": o.bps, "price": amt(o.price),
            "created": U64(o.created), "end": U64(o.end), "sold_bps": o.sold_bps,
            "min_part": if o.sold_bps < o.bps { o.min_part() } else { 0 },
            "escrow": amt(o.escrow()),
            "holders": o.holders.iter().map(|h| json!({"account": h.account, "bps": h.bps,
                "paid_in": amt(h.paid_in), "escrow": amt(h.escrow), "earned": amt(h.earned)}))
                .collect::<Vec<_>>(),
        })
    }

    // ------------------------------------------- owner: limits (feature 12)

    /// The owner's one business method: sets the price floor, the daily
    /// spending cap, the allowed payees and the revenue split. Needs 1
    /// yoctoNEAR (a full-access key of the owner). Refused while listed:
    /// buyers scanned these limits. After a sale the buyer holds the deed and
    /// the seller gets `NotOwner`.
    #[payable]
    pub fn set_limits(&mut self, limits: LimitsArgs) -> Value {
        near_sdk::assert_one_yocto();
        let caller = env::predecessor_account_id();
        // A52: no `base` keeps the current Base limits; clearing them takes
        // an explicit `{"daily_cap": "0", "payees": []}`.
        let base: Option<core::base::BaseLimits> = limits.base.clone().map(Into::into);
        self.m
            .set_limits(caller.as_str(), limits.into())
            .unwrap_or_else(|e| fail(e));
        if let Some(base) = base {
            self.m
                .set_base_limits(caller.as_str(), base)
                .unwrap_or_else(|e| fail(e));
        }
        let v = self.get_limits();
        emit("limits_set", v.clone());
        v
    }

    // --------------------------------------------------------- runner

    /// The runner pays `payee` out of the agent's treasury (procurement), only
    /// to an allowed payee and only up to the daily cap, and never while the
    /// agent is listed (A24). Refusals panic with `Listed: spending paused`,
    /// `PolicyViolation:Payee`, `PolicyViolation:DailyCap` or
    /// `InsufficientFunds`, plus the numbers; nothing changes.
    pub fn pay(&mut self, payee: AccountId, amount: U128) -> Value {
        if let Some(v) = self.runner_guard() {
            return v;
        }
        let now = env::block_timestamp();
        let spent_before = self.m.spent_today(now);
        let total = match self.m.spend(payee.as_str(), amount.0, now) {
            Ok(t) => t,
            Err(e) => {
                let l = &self.m.limits;
                let detail = match &e {
                    core::Error::PolicyViolation(core::Policy::DailyCap) => format!(
                        "{} + {} > daily cap {}",
                        usd(spent_before),
                        usd(amount.0),
                        usd(l.daily_cap)
                    ),
                    core::Error::PolicyViolation(core::Policy::Payee) => {
                        format!("{payee} is not an allowed payee")
                    }
                    core::Error::InsufficientFunds => {
                        format!("treasury {} < {}", usd(self.m.treasury), usd(amount.0))
                    }
                    _ => String::new(),
                };
                if detail.is_empty() {
                    env::panic_str(&e.to_string())
                }
                env::panic_str(&format!("{e}: {detail}"))
            }
        };
        let data = json!({
            "payee": payee, "amount": amt(amount.0), "spent_today": amt(total),
            "daily_cap": amt(self.m.limits.daily_cap), "treasury_left": amt(self.m.treasury),
            "owner_epoch": self.m.owner_epoch,
        });
        emit("treasury_pay", data.clone());
        self.send(OutKind::TreasuryPay, payee.as_str(), amount.0)
            .detach();
        json!({"ok": true, "paid": data})
    }

    /// Feature 5: the runner pays `to` on Base Sepolia from the agent's Chain
    /// Signatures wallet. The contract builds one USDC EIP-3009
    /// `transferWithAuthorization` digest from these fields (never a payload
    /// from the caller), inside the owner's Base limits, valid at most an
    /// hour (`valid_secs`, default 600), and asks the MPC signer to sign it.
    /// `on_base_signed` checks the signature against the wallet address and
    /// emits `base_auth_signed` with relay-ready `v, r, s` and calldata; any
    /// relayer broadcasts it. Until `valid_before` passes the agent cannot be
    /// listed or bought (A9). Attach `RUNNER_GAS_PAY_BASE`.
    pub fn pay_base(&mut self, to: String, amount: U128, valid_secs: Option<u64>) -> Value {
        if let Some(v) = self.runner_guard() {
            return v;
        }
        let now = env::block_timestamp();
        let spent_before = self.m.base.spent_today(self.m.owner_epoch, now);
        let plan = match self
            .m
            .begin_base_pay(&to, amount.0, valid_secs.unwrap_or(600), now)
        {
            Ok(p) => p,
            Err(e) => {
                let detail = match &e {
                    core::Error::PolicyViolation(core::Policy::BaseDailyCap) => format!(
                        ": {} + {} > Base daily cap {} USDC",
                        usd(spent_before),
                        usd(amount.0),
                        usd(self.m.base.limits.daily_cap)
                    ),
                    core::Error::PolicyViolation(core::Policy::BasePayee) => {
                        format!(": {to} is not an allowed Base payee")
                    }
                    core::Error::ValidityTooLong => {
                        format!(": at most {} s", core::base::MAX_VALID_SECS)
                    }
                    core::Error::TooManyPendingAuths => format!(
                        ": {} Base authorizations wait for the signer",
                        core::base::MAX_PENDING_AUTHS
                    ),
                    _ => String::new(),
                };
                env::panic_str(&format!("{e}{detail}"))
            }
        };
        let a = &plan.auth;
        emit(
            "base_sign_requested",
            json!({"auth_id": a.id, "owner_epoch": a.owner_epoch, "from": a.from, "to": a.to,
                   "value": amt(a.value), "valid_before": a.valid_before, "nonce": a.nonce,
                   "digest": a.digest, "mpc": MPC_SIGNER, "path": core::base::BASE_PATH}),
        );
        let args = json!({"request": {
            "path": core::base::BASE_PATH,
            "payload_v2": {"Ecdsa": verified::hex_lower(&plan.digest)},
            "domain_id": MPC_DOMAIN_SECP256K1,
        }});
        Promise::new(MPC_SIGNER.parse().expect("valid MPC account"))
            .function_call_weight(
                "sign",
                args.to_string().into_bytes(),
                NearToken::from_yoctonear(1),
                GAS_MPC_SIGN_MIN,
                near_sdk::GasWeight(1),
            )
            .then(
                Self::ext(env::current_account_id())
                    .with_static_gas(self.cb_gas(GAS_ON_BASE_SIGNED))
                    .with_unused_gas_weight(0)
                    .on_base_signed(a.id),
            )
            .detach();
        json!({"ok": true, "pending": true, "auth_id": a.id, "from": a.from, "to": a.to,
               "value": amt(a.value), "valid_before": a.valid_before})
    }

    /// MPC callback: the signature must recover to the wallet address. Then
    /// it is issued (low-s normalised for the token's `ecrecover`), otherwise
    /// the reserved cap is released and `sign_failed` says why (E22).
    #[private]
    pub fn on_base_signed(&mut self, auth_id: u64) -> Value {
        let Some(auth) = self.m.base.pending.get(&auth_id).cloned() else {
            return json!({"ok": false, "reason": "UnknownAuth"});
        };
        let bytes = match env::promise_result_checked(0, 2_048) {
            Ok(bytes) => bytes,
            Err(_) => {
                // The signer failed: no signature exists, release the cap.
                self.m.finish_base_pay(auth_id, false);
                let data = json!({"auth_id": auth_id, "reason": "SignFailed", "released": amt(auth.value),
                                  "open_auth_until": U64(self.m.open_auth_until)});
                emit("sign_failed", data.clone());
                return json!({"ok": false, "sign_failed": data});
            }
        };
        let sig = mpc_signature(&bytes);
        let digest: [u8; 32] = verified::hex_decode(&auth.digest)
            .and_then(|d| d.try_into().ok())
            .expect("stored digest");
        let checked = sig.and_then(|(r, s, rid)| {
            let mut rs = [0u8; 64];
            rs[..32].copy_from_slice(&r);
            rs[32..].copy_from_slice(&s);
            let pk = env::ecrecover(&digest, &rs, rid, false).ok_or("BadSignature")?;
            let h = env::keccak256_array(pk);
            if format!("0x{}", verified::hex_lower(&h[12..])) != auth.from {
                return Err("BaseAddressMismatch");
            }
            Ok(low_s(r, s, rid))
        });
        match checked {
            Ok((r, s, v)) => {
                self.m.finish_base_pay(auth_id, true);
                let from = core::base::parse_address(&auth.from).expect("valid");
                let to = core::base::parse_address(&auth.to).expect("valid");
                let nonce: [u8; 32] = verified::hex_decode(&auth.nonce)
                    .and_then(|d| d.try_into().ok())
                    .expect("stored nonce");
                let calldata = core::base::relay_calldata(
                    &from,
                    &to,
                    auth.value,
                    auth.valid_after,
                    auth.valid_before,
                    &nonce,
                    v,
                    &r,
                    &s,
                );
                let data = json!({
                    "auth_id": auth_id, "owner_epoch": auth.owner_epoch,
                    "chain_id": core::base::BASE_CHAIN_ID, "token": core::base::BASE_USDC,
                    "from": auth.from, "to": auth.to, "value": amt(auth.value),
                    "valid_after": auth.valid_after, "valid_before": auth.valid_before,
                    "nonce": auth.nonce, "digest": auth.digest,
                    "v": v, "r": format!("0x{}", verified::hex_lower(&r)),
                    "s": format!("0x{}", verified::hex_lower(&s)),
                    "calldata": format!("0x{}", verified::hex_lower(&calldata)),
                });
                emit("base_auth_signed", data.clone());
                json!({"ok": true, "auth": data})
            }
            Err(reason) if reason != "BadSignResponse" => {
                // A well-formed signature that is not the wallet's cannot
                // move the wallet's USDC: release the cap.
                self.m.finish_base_pay(auth_id, false);
                let data = json!({"auth_id": auth_id, "reason": reason, "released": amt(auth.value),
                                  "open_auth_until": U64(self.m.open_auth_until)});
                emit("sign_failed", data.clone());
                json!({"ok": false, "sign_failed": data})
            }
            Err(reason) => {
                // C29: the signer answered but the answer cannot be read. A
                // valid signature may still be in that receipt, so the cap
                // stays spent and the agent stays blocked until valid_before.
                self.m.finish_base_pay_unknown(auth_id);
                let data = json!({"auth_id": auth_id, "reason": reason, "released": amt(0),
                                  "open_auth_until": U64(self.m.open_auth_until)});
                emit("sign_unverified", data.clone());
                json!({"ok": false, "sign_unverified": data})
            }
        }
    }

    /// Runner liveness call. Like every runner method it must be a self-call
    /// signed by a key this contract added. A self-call signed by any other
    /// key proves a hidden key: the agent freezes, `backdoor_proven` is
    /// emitted and open escrow goes back to buyers (A23).
    pub fn heartbeat(&mut self) -> Value {
        match self.runner_guard() {
            Some(v) => v,
            None => json!({"ok": true, "owner_epoch": self.m.owner_epoch}),
        }
    }

    // --------------------------------------------------------- views

    /// Limits, treasury and today's spending (`limits.v1`).
    pub fn get_limits(&self) -> Value {
        let l = &self.m.limits;
        let now = env::block_timestamp();
        let spent = self.m.spent_today(now);
        json!({
            "schema": "limits.v1",
            "owner": self.m.owner,
            "owner_epoch": self.m.owner_epoch,
            "price_floor": amt(l.price_floor),
            "daily_cap": amt(l.daily_cap),
            "allowed_payees": l.allowed_payees,
            "split": {"owner_bps": l.owner_bps, "treasury_bps": l.treasury_bps},
            "treasury": amt(self.m.treasury),
            "spent_today": amt(spent),
            "cap_left_today": amt(l.daily_cap.saturating_sub(spent)),
            "day": now / core::DAY_NS,
        })
    }

    /// The Base Sepolia wallet (`base-wallet.v1`): address, limits, today's
    /// use, open and recent authorizations.
    pub fn get_base_wallet(&self) -> Value {
        let b = &self.m.base;
        let now = env::block_timestamp();
        let spent = b.spent_today(self.m.owner_epoch, now);
        json!({
            "schema": "base-wallet.v1",
            "agent": self.m.agent,
            "owner": self.m.owner,
            "owner_epoch": self.m.owner_epoch,
            "address": b.address,
            "chain_id": core::base::BASE_CHAIN_ID,
            "token": core::base::BASE_USDC,
            "domain_separator": format!("0x{}", verified::hex_lower(&core::base::usdc_domain())),
            "mpc": MPC_SIGNER,
            "path": core::base::BASE_PATH,
            "daily_cap": amt(b.limits.daily_cap),
            "payees": b.limits.payees,
            "spent_today": amt(spent),
            "cap_left_today": amt(b.limits.daily_cap.saturating_sub(spent)),
            "open_auth_until": U64(self.m.open_auth_until),
            "pending": b.pending.values().collect::<Vec<_>>(),
            "signed": b.signed,
        })
    }

    pub fn get_earnings(&self) -> core::Earnings {
        self.m.earnings(env::block_timestamp())
    }

    /// Day rows, oldest first, `limit` at most 100 (A47). `get_earnings`
    /// shows the newest 31; this pages through all kept rows.
    pub fn get_earnings_days(
        &self,
        from_index: Option<u64>,
        limit: Option<u64>,
    ) -> Vec<core::meter::DayRow> {
        self.m.meter.days(
            &self.m.store,
            from_index.unwrap_or(0),
            limit.unwrap_or(100).min(100),
        )
    }

    /// NEP-145 style: register `account_id` (default: the caller) as a
    /// payer with its own row, for `PAYER_ROW_DEPOSIT` NEAR (A49). The row
    /// is what lets the agent's earnings count it as a distinct payer;
    /// payments without a row are pooled. Excess is refunded; an account
    /// already registered gets the whole deposit back. There is no cap on
    /// rows (A50): each pays for its own storage. The deposit also pays for
    /// the payer's own `PAYER_SIGS` verified-signature slots (A54).
    #[payable]
    pub fn storage_deposit(
        &mut self,
        account_id: Option<AccountId>,
        registration_only: Option<bool>,
    ) -> Value {
        let _ = registration_only;
        let who = account_id.unwrap_or_else(env::predecessor_account_id);
        let paid = env::attached_deposit();
        let fresh = self.m.store_payer_missing(who.as_str());
        let refund = if fresh {
            if paid < PAYER_ROW_DEPOSIT {
                env::panic_str("StorageDepositTooSmall");
            }
            if !self.m.register_payer(who.as_str()) {
                env::panic_str("AlreadyRegistered");
            }
            write_deposit(
                &who,
                &DepositRecord {
                    depositor: env::predecessor_account_id().to_string(),
                    at: env::block_timestamp(),
                    amount: PAYER_ROW_DEPOSIT.as_yoctonear(),
                },
            );
            self.payer_deposits += PAYER_ROW_DEPOSIT.as_yoctonear();
            emit(
                "payer_registered",
                json!({"account": who, "by": env::predecessor_account_id()}),
            );
            paid.saturating_sub(PAYER_ROW_DEPOSIT)
        } else {
            paid
        };
        if !refund.is_zero() {
            Promise::new(env::predecessor_account_id())
                .transfer(refund)
                .detach();
        }
        self.storage_balance()
    }

    pub fn storage_balance_of(&self, account_id: AccountId) -> Option<Value> {
        (!self.m.store_payer_missing(account_id.as_str())).then(|| self.storage_balance())
    }

    /// NEP-145: remove the caller's own row, if it never had a payment, and
    /// refund its deposit to the caller (A50). `false` if the caller has no
    /// row. A row with payments is part of the earnings record and stays
    /// (`PayerHasPayments`, also with `force`). Needs 1 yoctoNEAR.
    #[payable]
    pub fn storage_unregister(&mut self, force: Option<bool>) -> bool {
        near_sdk::assert_one_yocto();
        let _ = force;
        let who = env::predecessor_account_id();
        if self.m.store_payer_missing(who.as_str()) {
            return false;
        }
        self.m
            .unregister_payer(who.as_str())
            .unwrap_or_else(|e| env::panic_str(e));
        let rec = take_deposit(&who);
        let held = rec.map_or(0, |r| r.amount);
        self.payer_deposits -= held;
        let amount = held + 1;
        Promise::new(who.clone())
            .transfer(NearToken::from_yoctonear(amount))
            .detach();
        emit(
            "payer_unregistered",
            json!({"account": who, "refund_to": who, "amount": amount.to_string()}),
        );
        true
    }

    /// NEP-145: nothing above the row's price is ever held, so there is
    /// nothing to withdraw; returns the caller's balance. Needs 1 yoctoNEAR.
    #[payable]
    pub fn storage_withdraw(&mut self, amount: Option<NearToken>) -> Value {
        near_sdk::assert_one_yocto();
        let who = env::predecessor_account_id();
        if self.m.store_payer_missing(who.as_str()) {
            env::panic_str("NotRegistered");
        }
        if amount.is_some_and(|a| !a.is_zero()) {
            env::panic_str("NothingToWithdraw");
        }
        self.storage_balance()
    }

    /// Anyone may remove a row that has had no payment for
    /// `EMPTY_ROW_TTL_NS` after its deposit; the deposit goes back to whoever
    /// paid it (A50). So rows registered for accounts that never pay do not
    /// stay in the payer count for good.
    pub fn reclaim_payer_row(&mut self, account_id: AccountId) -> Value {
        let Some(rec) = read_deposit(&account_id) else {
            env::panic_str("NotRegistered");
        };
        if env::block_timestamp() < rec.at.saturating_add(EMPTY_ROW_TTL_NS) {
            env::panic_str("RowTooYoung");
        }
        self.m
            .unregister_payer(account_id.as_str())
            .unwrap_or_else(|e| env::panic_str(e));
        take_deposit(&account_id);
        self.payer_deposits -= rec.amount;
        let to: AccountId = rec.depositor.parse().expect("stored account id");
        Promise::new(to.clone())
            .transfer(NearToken::from_yoctonear(rec.amount))
            .detach();
        let ev = json!({"account": account_id, "refund_to": to,
                        "amount": rec.amount.to_string(), "reclaimed": true});
        emit("payer_unregistered", ev.clone());
        ev
    }

    pub fn storage_balance_bounds(&self) -> Value {
        let d = PAYER_ROW_DEPOSIT.as_yoctonear().to_string();
        json!({"min": d, "max": d})
    }

    /// One payer's row (A47); `None` if the payer has no row of its own.
    pub fn get_payer(&self, account_id: AccountId) -> Option<core::meter::PayerStats> {
        use core::MeterStore;
        self.m.store.payer(account_id.as_str())
    }

    pub fn known_keys(&self) -> Vec<String> {
        self.m.known_keys.iter().cloned().collect()
    }

    pub fn known_keys_hash(&self) -> String {
        self.m.known_keys_hash()
    }

    pub fn get_listing(&self) -> Option<Value> {
        self.m.listing.as_ref().map(|l| self.listing_json(l))
    }

    /// Sales by id, oldest first: ids `from_index + 1 ..`, at most `limit`
    /// (default 20, at most 50). Without `from_index`, the newest `limit`.
    /// Open earn-outs come from the state, finalized ones from the history
    /// (A47).
    pub fn get_sales(&self, from_index: Option<u64>, limit: Option<u64>) -> Vec<Value> {
        let last = self.m.next_sale_id.saturating_sub(1);
        let limit = limit.unwrap_or(20).min(50);
        let from = from_index.unwrap_or(last.saturating_sub(limit));
        (from + 1..=last.min(from.saturating_add(limit)))
            .filter_map(|id| self.m.sale(id))
            .map(|s| self.sale_json(&s))
            .collect()
    }

    pub fn get_claimable(&self, account_id: AccountId) -> U128 {
        U128(self.m.claimable_of(account_id.as_str()))
    }

    pub fn get_status(&self) -> Value {
        json!({
            "schema": "agent-status.v1",
            "agent": self.m.agent,
            "owner": self.m.owner,
            "owner_epoch": self.m.owner_epoch,
            "status": self.m.status,
            "listing_version": self.m.listing_version,
            "paused": self.m.paused,
            "frozen": self.m.frozen,
            "token": self.token,
            "admin": self.admin,
            "signers": self.signers,
            "terms_min": self.m.terms_min,
            "terms_config": self.m.terms_min.config(),
            "paid_to_total": self.m.paid_to_total(),
            "open_sales": self.m.sales.len(),
            "closed_sales": self.m.closed_sales,
            "claimable_accounts": self.m.claimable_accounts,
            "tracked_payers": self.m.meter.tracked_payers,
            "empty_payer_rows": self.m.meter.empty_payers,
            "past_owners": self.m.past_owners,
            "paid_to_owner": self.m.paid_to_this_owner(),
            "incoming_keys": self.m.incoming,
            "retiring_keys": self.m.retiring,
            "known_keys": self.m.known_keys,
            "known_keys_hash": self.m.known_keys_hash(),
            "open_approvals": self.m.approvals.len(),
            "held": amt(self.m.held),
            "owner_free": amt(self.m.owner_free),
            "treasury": amt(self.m.treasury),
            "escrow_left": amt(self.m.escrow_left()),
            "claimable_total": amt(self.m.claimable_total()),
            "shares_bps": self.m.shares_bps(),
            "share_escrow": amt(self.m.share_escrow),
            "solvent": self.m.solvent(),
            "near_storage": self.near_storage(),
        })
    }

    /// NEAR storage solvency (A46): what the account stakes for storage now,
    /// what the bounded signature window may still add, and whether the
    /// balance covers both plus `NEAR_RESERVE`. The scan blocks an agent
    /// that is not solvent.
    fn near_storage(&self) -> Value {
        let cost = env::storage_byte_cost();
        let staked = cost.saturating_mul(env::storage_usage() as u128);
        let growth = self.sig_growth();
        let balance = env::account_balance();
        let need = staked.saturating_add(growth).saturating_add(NEAR_RESERVE);
        json!({
            "usage_bytes": env::storage_usage(),
            "staked": staked.as_yoctonear().to_string(),
            "balance": balance.as_yoctonear().to_string(),
            "sig_entries": self.used_sigs.len(),
            "max_sig_entries": verified::MAX_SIGS,
            "payer_entries": self.m.meter.tracked_payers,
            "empty_payer_entries": self.m.meter.empty_payers,
            "payer_deposits": self.payer_deposits.to_string(),
            "day_entries": self.m.meter.days_total(),
            "max_day_entries": core::meter::MAX_DAYS,
            "growth_reserve": growth.as_yoctonear().to_string(),
            "solvent": balance >= need,
        })
    }

    /// Storage outsiders may still add for free: signature ids (A46) and
    /// day rows (A47). Each table is capped, so this is a bound; the
    /// owner cannot withdraw it, so outsiders can never make the agent
    /// unable to pay for its storage.
    fn sig_growth(&self) -> NearToken {
        let sigs = (verified::MAX_SIGS - self.used_sigs.len()) as u128 * SIG_ENTRY_BYTES;
        // Payer rows are paid by their `storage_deposit` (A49).
        let days = (core::meter::MAX_DAYS as u128)
            .saturating_sub(self.m.meter.days_total() as u128)
            * DAY_ENTRY_BYTES;
        env::storage_byte_cost().saturating_mul(sigs + days)
    }

    pub fn near_withdrawable(&self) -> NearToken {
        let storage = env::storage_byte_cost().saturating_mul(env::storage_usage() as u128);
        // Payer deposits are subtracted whole, although part of them is
        // already in `storage`: conservative, so a refund or a payer's
        // signature slots are always covered (A50).
        env::account_balance()
            .saturating_sub(storage)
            .saturating_sub(self.sig_growth())
            .saturating_sub(NEAR_RESERVE)
            .saturating_sub(NearToken::from_yoctonear(self.payer_deposits))
    }

    // --------------------------------------------------------- NEP-171 deed

    pub fn nft_metadata(&self) -> NFTContractMetadata {
        NFTContractMetadata {
            spec: NFT_METADATA_SPEC.to_string(),
            name: "Agent Exit deed".into(),
            symbol: "AXDEED".into(),
            icon: None,
            base_uri: None,
            reference: None,
            reference_hash: None,
        }
    }

    pub fn nft_token(&self, token_id: TokenId) -> Option<Token> {
        (token_id == DEED_ID).then(|| self.deed())
    }

    pub fn nft_total_supply(&self) -> U128 {
        U128(1)
    }

    pub fn nft_tokens(&self, from_index: Option<U128>, limit: Option<u64>) -> Vec<Token> {
        if from_index.map(|i| i.0).unwrap_or(0) > 0 || limit == Some(0) {
            return vec![];
        }
        vec![self.deed()]
    }

    pub fn nft_supply_for_owner(&self, account_id: AccountId) -> U128 {
        U128((account_id.as_str() == self.m.owner) as u128)
    }

    pub fn nft_tokens_for_owner(
        &self,
        account_id: AccountId,
        from_index: Option<U128>,
        limit: Option<u64>,
    ) -> Vec<Token> {
        if account_id.as_str() == self.m.owner {
            self.nft_tokens(from_index, limit)
        } else {
            vec![]
        }
    }

    /// Moving the deed outside `buy()` runs the handover without the money:
    /// every runner key is deleted and the agent pauses until the new owner
    /// sets a runner key (E24, A16).
    #[payable]
    pub fn nft_transfer(
        &mut self,
        receiver_id: AccountId,
        token_id: TokenId,
        approval_id: Option<u64>,
        memo: Option<String>,
    ) {
        near_sdk::assert_one_yocto();
        self.move_deed(&receiver_id, &token_id, approval_id, memo.as_deref());
    }

    #[payable]
    pub fn nft_transfer_call(
        &mut self,
        receiver_id: AccountId,
        token_id: TokenId,
        approval_id: Option<u64>,
        memo: Option<String>,
        msg: String,
    ) -> PromiseOrValue<bool> {
        near_sdk::assert_one_yocto();
        let previous = self.owner_id();
        self.move_deed(&receiver_id, &token_id, approval_id, memo.as_deref());
        ext_nft_receiver::ext(receiver_id.clone())
            .with_static_gas(GAS_NFT_ON_TRANSFER)
            .nft_on_transfer(
                env::predecessor_account_id(),
                previous.clone(),
                token_id.clone(),
                msg,
            )
            .then(
                Self::ext(env::current_account_id())
                    .with_static_gas(self.cb_gas(GAS_NFT_RESOLVE))
                    .nft_resolve_transfer(previous, receiver_id, token_id, None),
            )
            .into()
    }

    /// NEP-171 resolve: if the receiver asked to return the deed and still
    /// holds it, it goes back (keys stay deleted; the agent stays paused).
    #[private]
    pub fn nft_resolve_transfer(
        &mut self,
        previous_owner_id: AccountId,
        receiver_id: AccountId,
        token_id: TokenId,
        approved_account_ids: Option<HashMap<AccountId, u64>>,
    ) -> bool {
        let _ = approved_account_ids;
        let give_back = match env::promise_result_checked(0, 16) {
            Ok(v) => serde_json::from_slice::<bool>(&v).unwrap_or(true),
            Err(_) => true,
        };
        if !give_back || self.m.owner != receiver_id.as_str() {
            return true;
        }
        // A35: if the return is refused (the receiver started a key batch or
        // a sale inside nft_on_transfer), the receiver keeps the deed and the
        // method says so; no NEP-171 event claims a move that did not happen.
        let keys = match self.m.transfer_deed(
            receiver_id.as_str(),
            previous_owner_id.as_str(),
            None,
            env::block_timestamp(),
        ) {
            Ok(keys) => keys,
            Err(e) => {
                emit(
                    "deed_return_refused",
                    json!({"owner": receiver_id, "previous_owner": previous_owner_id,
                           "reason": e.to_string()}),
                );
                return true;
            }
        };
        self.delete_keys(&keys);
        NftTransfer {
            old_owner_id: &receiver_id,
            new_owner_id: &previous_owner_id,
            token_ids: &[&token_id],
            authorized_id: None,
            memo: Some("agent_exit:returned"),
        }
        .emit();
        false
    }

    #[payable]
    pub fn nft_approve(
        &mut self,
        token_id: TokenId,
        account_id: AccountId,
        msg: Option<String>,
    ) -> Option<Promise> {
        require!(token_id == DEED_ID, "UnknownToken");
        require!(
            env::attached_deposit() >= NearToken::from_yoctonear(1),
            "Requires attached deposit"
        );
        let caller = env::predecessor_account_id();
        let id = self
            .m
            .approve(caller.as_str(), account_id.as_str())
            .unwrap_or_else(|e| fail(e));
        msg.map(|msg| {
            ext_nft_approval_receiver::ext(account_id)
                .with_static_gas(GAS_NFT_ON_APPROVE)
                .nft_on_approve(token_id, caller, id, msg)
        })
    }

    #[payable]
    pub fn nft_revoke(&mut self, token_id: TokenId, account_id: AccountId) {
        near_sdk::assert_one_yocto();
        require!(token_id == DEED_ID, "UnknownToken");
        let caller = env::predecessor_account_id();
        self.m
            .revoke(caller.as_str(), Some(account_id.as_str()))
            .unwrap_or_else(|e| fail(e));
    }

    #[payable]
    pub fn nft_revoke_all(&mut self, token_id: TokenId) {
        near_sdk::assert_one_yocto();
        require!(token_id == DEED_ID, "UnknownToken");
        let caller = env::predecessor_account_id();
        self.m
            .revoke(caller.as_str(), None)
            .unwrap_or_else(|e| fail(e));
    }

    pub fn nft_is_approved(
        &self,
        token_id: TokenId,
        approved_account_id: AccountId,
        approval_id: Option<u64>,
    ) -> bool {
        if token_id != DEED_ID {
            return false;
        }
        match (
            self.m.approvals.get(approved_account_id.as_str()),
            approval_id,
        ) {
            (Some(id), Some(given)) => *id == given,
            (Some(_), None) => true,
            _ => false,
        }
    }
}

/// Parse the MPC signer's `sign` result: `{"big_r": {"affine_point": hex33},
/// "s": {"scalar": hex32}, "recovery_id": n}`, optionally tagged
/// (`"scheme": "Secp256k1"` or wrapped in `{"Secp256k1": ...}`).
pub fn mpc_signature(bytes: &[u8]) -> Result<([u8; 32], [u8; 32], u8), &'static str> {
    let v: Value = serde_json::from_slice(bytes).map_err(|_| "BadSignResponse")?;
    let v = v.get("Secp256k1").cloned().unwrap_or(v);
    let r = v["big_r"]["affine_point"]
        .as_str()
        .and_then(verified::hex_decode)
        .filter(|p| p.len() == 33 && (p[0] == 2 || p[0] == 3))
        .ok_or("BadSignResponse")?;
    let s = v["s"]["scalar"]
        .as_str()
        .and_then(verified::hex_decode)
        .filter(|s| s.len() == 32)
        .ok_or("BadSignResponse")?;
    let rid = v["recovery_id"]
        .as_u64()
        .filter(|r| *r <= 1)
        .ok_or("BadSignResponse")? as u8;
    Ok((
        r[1..].try_into().expect("32"),
        s.try_into().expect("32"),
        rid,
    ))
}

/// The deed's live picture: a data-URI SVG with the agent's counted,
/// Verified and flagged revenue and the share of earnings pledged.
pub fn deed_svg(
    agent: &str,
    counted: &str,
    verified: &str,
    flagged: &str,
    shares_bps: u16,
    frozen: bool,
) -> String {
    let esc = |t: &str| {
        t.chars()
            .filter(|c| c.is_ascii_alphanumeric() || ".-_ ".contains(*c))
            .collect::<String>()
    };
    let state = if frozen { "FROZEN" } else { "active" };
    let svg = format!(
        "<svg xmlns='http://www.w3.org/2000/svg' width='400' height='240' viewBox='0 0 400 240'>\
<rect width='400' height='240' rx='16' fill='%230b1510'/>\
<text x='24' y='44' fill='%233ddc97' font-family='monospace' font-size='16'>AGENT EXIT DEED</text>\
<text x='24' y='78' fill='white' font-family='monospace' font-size='15'>{}</text>\
<text x='24' y='120' fill='white' font-family='monospace' font-size='14'>counted  {}</text>\
<text x='24' y='146' fill='white' font-family='monospace' font-size='14'>verified {}</text>\
<text x='24' y='172' fill='white' font-family='monospace' font-size='14'>flagged  {}</text>\
<text x='24' y='210' fill='%23a0b0a8' font-family='monospace' font-size='13'>{}% to investors / {}</text></svg>",
        esc(agent),
        esc(counted),
        esc(verified),
        esc(flagged),
        shares_bps as f64 / 100.0,
        state
    );
    format!("data:image/svg+xml;utf8,{svg}")
}

/// secp256k1 group order.
const SECP_N: [u8; 32] = [
    0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xfe,
    0xba, 0xae, 0xdc, 0xe6, 0xaf, 0x48, 0xa0, 0x3b, 0xbf, 0xd2, 0x5e, 0x8c, 0xd0, 0x36, 0x41, 0x41,
];
const SECP_HALF_N: [u8; 32] = [
    0x7f, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff,
    0x5d, 0x57, 0x6e, 0x73, 0x57, 0xa4, 0x50, 0x1d, 0xdf, 0xe9, 0x2f, 0x46, 0x68, 0x1b, 0x20, 0xa0,
];

/// USDC's `ecrecover` refuses high-s signatures: use (r, n - s) and flip v.
/// Returns (r, s, v) with v in {27, 28}.
pub fn low_s(r: [u8; 32], s: [u8; 32], rid: u8) -> ([u8; 32], [u8; 32], u8) {
    if s <= SECP_HALF_N {
        return (r, s, 27 + rid);
    }
    let mut out = [0u8; 32];
    let mut borrow = 0i16;
    for i in (0..32).rev() {
        let d = SECP_N[i] as i16 - s[i] as i16 - borrow;
        borrow = (d < 0) as i16;
        out[i] = (d + 256 * borrow) as u8;
    }
    (r, out, 27 + (rid ^ 1))
}

fn norm_address(a: &str) -> String {
    let a = a.trim().to_ascii_lowercase();
    let a = a.strip_prefix("0x").unwrap_or(&a).to_string();
    if a.len() != 40 || !a.bytes().all(|c| c.is_ascii_hexdigit()) {
        env::panic_str("BadAddress");
    }
    format!("0x{a}")
}

impl AgentExit {
    /// Bytes of the contract state blob (`STATE`), which every call reads and
    /// writes: the machine, the signer list and the account ids. Not the
    /// account's storage: that also counts the contract code and the
    /// `used_sigs` entries, which no callback reads (A43).
    pub fn state_bytes(&self) -> u64 {
        near_sdk::borsh::object_length(self).unwrap_or(0) as u64
    }

    /// Gas to read and write the state blob once (A33, A43).
    pub fn state_gas(&self) -> Gas {
        Gas::from_gas(self.state_bytes().saturating_mul(GAS_PER_STATE_BYTE))
    }

    /// A callback's gas: its own work plus the state term, so a callback does
    /// not run out of gas as outsiders' payments grow the state (A33).
    pub fn cb_gas(&self, base: Gas) -> Gas {
        base.saturating_add(self.state_gas())
    }

    fn only_admin(&self) {
        if env::predecessor_account_id() != self.admin {
            env::panic_str("NotAdmin");
        }
    }

    fn owner_id(&self) -> AccountId {
        self.m.owner.parse().expect("valid owner")
    }

    fn send(&mut self, kind: OutKind, to: &str, amount: Amount) -> Promise {
        self.send_by(kind, to, amount, None)
    }

    /// `by`: who started it (owner withdrawals), for the credit-back (A38).
    fn send_by(
        &mut self,
        kind: OutKind,
        to: &str,
        amount: Amount,
        by: Option<AccountId>,
    ) -> Promise {
        let to: AccountId = to.parse().expect("valid account");
        ext_ft_core::ext(self.token.clone())
            .with_attached_deposit(NearToken::from_yoctonear(1))
            .with_static_gas(GAS_FT_TRANSFER)
            .ft_transfer(
                to.clone(),
                U128(amount),
                Some(format!("agent_exit:{kind:?}")),
            )
            .then(
                Self::ext(env::current_account_id())
                    .with_static_gas(self.cb_gas(GAS_ON_TRANSFER_OUT))
                    .on_transfer_out(kind, to, U128(amount), by),
            )
    }

    /// Returns `Some(response)` when the call must stop (backdoor proven).
    fn runner_guard(&mut self) -> Option<Value> {
        let signer_pk = String::from(&env::signer_account_pk());
        match self
            .m
            .runner_guard(env::predecessor_account_id().as_str(), &signer_pk)
        {
            Ok(RunnerCheck::Ok) => None,
            Ok(RunnerCheck::BackdoorProven { returned }) => {
                let data = json!({
                    "signer_pk": signer_pk,
                    "returned": returned
                        .iter()
                        .map(|(id, b, a)| json!({"sale_id": id, "buyer": b, "amount": amt(*a)}))
                        .collect::<Vec<_>>(),
                });
                emit("backdoor_proven", data.clone());
                Some(json!({"ok": false, "frozen": true, "backdoor_proven": data}))
            }
            Err(e) => fail(e),
        }
    }

    fn move_deed(
        &mut self,
        receiver_id: &AccountId,
        token_id: &str,
        approval_id: Option<u64>,
        memo: Option<&str>,
    ) {
        require!(token_id == DEED_ID, "UnknownToken");
        let caller = env::predecessor_account_id();
        let old_owner = self.owner_id();
        let keys = self
            .m
            .transfer_deed(
                caller.as_str(),
                receiver_id.as_str(),
                approval_id,
                env::block_timestamp(),
            )
            .unwrap_or_else(|e| fail(e));
        self.delete_keys(&keys);
        NftTransfer {
            old_owner_id: &old_owner,
            new_owner_id: receiver_id,
            token_ids: &[token_id],
            authorized_id: (caller != old_owner).then_some(caller.as_ref()),
            memo,
        }
        .emit();
        emit(
            "deed_moved",
            json!({"from": old_owner, "to": receiver_id, "deleted_keys": keys, "paused": true,
                   "owner_epoch": self.m.owner_epoch}),
        );
    }

    /// Delete `keys`, each in its own receipt (a key that is already gone
    /// fails only its own receipt, never the others), and confirm in
    /// `on_keys_deleted`; until then they are `retiring` and a signature by
    /// them is `NotRunner`, not a backdoor (A25, A33).
    fn delete_keys(&self, keys: &[String]) {
        let Some(first) = keys.first() else {
            return;
        };
        let me = env::current_account_id();
        let mut all = Promise::new(me.clone()).delete_key(pk(first));
        for k in &keys[1..] {
            all = all.and(Promise::new(me.clone()).delete_key(pk(k)));
        }
        all.then(
            Self::ext(me)
                .with_static_gas(self.cb_gas(GAS_ON_KEYS))
                .on_keys_deleted(keys.to_vec()),
        )
        .detach();
    }

    fn deed(&self) -> Token {
        let e = self.m.earnings(env::block_timestamp());
        let extra = json!({
            "schema": "deed-extra.v1",
            "agent": self.m.agent,
            "status": self.m.status,
            "owner_epoch": self.m.owner_epoch,
            "listing_version": self.m.listing_version,
            "earnings": {"gross": amt(e.gross), "counted": amt(e.counted), "verified": amt(e.verified),
                         "flagged": amt(e.flagged), "counted_payers": e.counted_payers},
            "sales": self.m.sales.len() as u64 + self.m.closed_sales,
            "escrow_left": amt(self.m.escrow_left()),
            "frozen": self.m.frozen,
            "known_keys_hash": self.m.known_keys_hash(),
            "limits": {"daily_cap": amt(self.m.limits.daily_cap),
                       "allowed_payees": self.m.limits.allowed_payees.len(),
                       "treasury_bps": self.m.limits.treasury_bps},
            "treasury": amt(self.m.treasury),
            "shares_bps": self.m.shares_bps(),
            "last_scan": Value::Null,
        });
        // The deed's picture is drawn from the contract's own numbers on
        // every view, so a wallet shows what the agent earns now.
        let media = deed_svg(
            &self.m.agent,
            &usd(e.counted),
            &usd(e.verified),
            &usd(e.flagged),
            self.m.shares_bps(),
            self.m.frozen,
        );
        Token {
            token_id: DEED_ID.into(),
            owner_id: self.owner_id(),
            metadata: Some(TokenMetadata {
                title: Some(format!("Agent Exit deed: {}", self.m.agent)),
                description: Some(
                    "Ownership deed of an AI agent account. The holder is the owner: sets the runner key and limits, receives the revenue."
                        .into(),
                ),
                media: Some(media),
                media_hash: None,
                copies: Some(1),
                issued_at: None,
                expires_at: None,
                starts_at: None,
                updated_at: None,
                extra: Some(extra.to_string()),
                reference: None,
                reference_hash: None,
            }),
            approved_account_ids: Some(
                self.m
                    .approvals
                    .iter()
                    .map(|(a, id)| (a.parse().expect("valid"), *id))
                    .collect(),
            ),
        }
    }

    fn listing_json(&self, l: &core::Listing) -> Value {
        let (now, escrow) = core::money::split_price(l.price, l.terms.earnout_bps);
        json!({
            "schema": "listing.v1",
            "agent": self.m.agent,
            "seller": l.seller,
            "listing_version": l.version,
            "price": amt(l.price),
            "paid_now": amt(now),
            "escrow": amt(escrow),
            "terms": l.terms,
            "config": l.config,
            "terms_min": self.m.terms_min,
            "terms_config": self.m.terms_min.config(),
            "code_hash": l.code_hash,
            "known_keys": self.m.known_keys,
            "known_keys_hash": self.m.known_keys_hash(),
            "token": self.token,
            "listed_at": U64(l.listed_at),
            "treasury_snapshot": amt(l.treasury_snapshot),
            // SH7: the buyer inherits these; the scan discloses them.
            "shares_bps": self.m.shares_bps(),
            "shares": self.get_shares(),
        })
    }

    fn storage_balance(&self) -> Value {
        json!({"total": PAYER_ROW_DEPOSIT.as_yoctonear().to_string(), "available": "0"})
    }

    fn sale_json(&self, s: &core::Sale) -> Value {
        json!({
            "sale_id": s.id, "seller": s.seller, "buyer": s.buyer,
            "price": amt(s.price), "paid_now": amt(s.paid_now), "escrow": amt(s.escrow),
            "released": amt(s.released), "returned": amt(s.returned), "remaining": amt(s.remaining()),
            "window_end": U64(s.window_end), "finalized": s.finalized, "cut": s.cut,
        })
    }
}

// ------------------------------------------------ payer deposits (A50)

/// Who paid for a payer row, when, and how much (refunded on removal).
#[derive(near_sdk::borsh::BorshSerialize, near_sdk::borsh::BorshDeserialize)]
#[borsh(crate = "near_sdk::borsh")]
struct DepositRecord {
    depositor: String,
    at: u64,
    amount: u128,
}

fn deposit_key(who: &AccountId) -> Vec<u8> {
    [DEPOSIT_PREFIX, who.as_bytes()].concat()
}

fn write_deposit(who: &AccountId, r: &DepositRecord) {
    env::storage_write(
        &deposit_key(who),
        &near_sdk::borsh::to_vec(r).expect("borsh"),
    );
}

fn read_deposit(who: &AccountId) -> Option<DepositRecord> {
    env::storage_read(&deposit_key(who))
        .map(|b| near_sdk::borsh::from_slice(&b).expect("deposit record"))
}

fn take_deposit(who: &AccountId) -> Option<DepositRecord> {
    let r = read_deposit(who);
    env::storage_remove(&deposit_key(who));
    r
}
