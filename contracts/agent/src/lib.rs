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
use near_contract_standards::non_fungible_token::events::NftTransfer;
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
mod review2_regressions;
#[cfg(test)]
mod review_regressions;
#[cfg(any(test, feature = "sim"))]
pub mod sim;
#[cfg(test)]
mod tests;
pub mod verified;

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

/// Methods the runner's function-call key may call (receiver = the agent).
pub const RUNNER_METHODS: &str = "heartbeat,accept_job,pay,pay_base";
/// Gas budget of the runner's function-call key. The node checks it against
/// the attached gas at a pessimistic gas price before every call, so the
/// runner attaches only what a call needs (`RUNNER_GAS_*`). 5 NEAR covers
/// thousands of heartbeats; the owner refreshes it by rotating the key.
pub const RUNNER_ALLOWANCE: NearToken = NearToken::from_near(5);
/// Gas a runner attaches: `heartbeat` (state read and write only).
pub const RUNNER_GAS_HEARTBEAT: Gas = Gas::from_tgas(30);
/// Gas a runner attaches: `pay` (ft_transfer 10 TGas + callback 10 TGas + execution).
pub const RUNNER_GAS_PAY: Gas = Gas::from_tgas(60);
/// NEAR kept on the account on top of storage staking (E17).
pub const NEAR_RESERVE: NearToken = NearToken::from_near(1);
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
    m: Machine,
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
    used_sigs: near_sdk::store::LookupSet<[u8; 32]>,
}

fn emit(event: &str, data: Value) {
    env::log_str(&format!(
        "EVENT_JSON:{}",
        json!({"standard": "agent_exit", "version": "1.0.0", "event": event, "data": [data]})
    ));
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

/// Gas to read and write the whole state once (A33).
fn state_gas() -> Gas {
    Gas::from_gas(env::storage_usage().saturating_mul(GAS_PER_STATE_BYTE))
}

/// A callback's gas: its own work plus the state term, so a callback does
/// not run out of gas as outsiders' payments grow the state (A33).
pub fn cb_gas(base: Gas) -> Gas {
    base.saturating_add(state_gas())
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
    #[init]
    #[private]
    pub fn new(owner_id: AccountId) -> Self {
        let mut m = Machine::new(env::current_account_id().as_str(), owner_id.as_str());
        m.terms_min = TERMS_MIN;
        Self {
            m,
            token: PAYMENT_TOKEN.parse().expect("valid token"),
            admin: REGISTRY_ADMIN.parse().expect("valid admin"),
            signers: BTreeSet::new(),
            used_sigs: near_sdk::store::LookupSet::new(b"u"),
        }
    }

    // ------------------------------------------------------- admin: signers

    /// Admin adds a NEAR AI Cloud signing address. Payments verified before
    /// keep their badge whatever happens to the list later (E14).
    pub fn add_signer(&mut self, address: String) {
        self.only_admin();
        let a = norm_address(&address);
        self.signers.insert(a.clone());
        emit("signer_added", json!({"address": a}));
    }

    pub fn remove_signer(&mut self, address: String) {
        self.only_admin();
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
                .with_static_gas(cb_gas(GAS_ON_KEYS))
                .on_runner_key(old, deleted, new_key),
        )
    }

    /// Callback of the rotation batch (all-or-nothing): success makes the new
    /// key the runner key, failure restores the old one (A25).
    #[private]
    pub fn on_runner_key(
        &mut self,
        old: Vec<String>,
        deleted: Vec<String>,
        new_key: String,
    ) -> bool {
        let ok = promise_ok();
        self.m.runner_key_done(&old, &deleted, &new_key, ok);
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
                let check = nearai.as_ref().map(|p| {
                    verified::check(
                        p,
                        self.m.agent.as_str(),
                        sender_id.as_str(),
                        amount.0,
                        job_id.as_deref(),
                        &self.signers,
                        &self.used_sigs,
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
                    } => {
                        if let (true, Some(verified::Outcome::Verified { sig_id, .. })) =
                            (verified, &check)
                        {
                            self.used_sigs.insert(*sig_id);
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
                                "seller_earnout_release": amt(earnout),
                            }),
                        );
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
            Msg::Buy {
                listing_version,
                known_keys_hash,
                buyer_runner_pk,
            } => {
                let left = env::prepaid_gas().saturating_sub(env::used_gas());
                // The callback and the payout callback each read and write
                // the state, so the bar grows with it (A33).
                let gas_ok = left
                    >= MIN_BUY_GAS
                        .saturating_add(state_gas())
                        .saturating_add(state_gas());
                let buyer_key = String::from(&buyer_runner_pk);
                let plan = match self.m.begin_buy(
                    sender_id.as_str(),
                    amount.0,
                    listing_version,
                    &known_keys_hash,
                    &buyer_key,
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
                            .with_static_gas(cb_gas(GAS_ON_HANDOVER))
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
        let res = self
            .m
            .finish_buy(sale_id, ok, now)
            .unwrap_or_else(|e| fail(e));
        match res {
            BuyResult::RolledBack { unused } => {
                emit(
                    "sale_rolled_back",
                    json!({"sale_id": sale_id, "refund": amt(unused)}),
                );
                U128(unused)
            }
            BuyResult::Done {
                sale,
                seller,
                payout,
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
                        "escrow": amt(sale.escrow), "overpay_refund": amt(unused),
                        "seller_revenue_claimable": amt(seller_revenue),
                        "owner_epoch": self.m.owner_epoch,
                        "window_end": U64(sale.window_end),
                        "known_keys": self.m.known_keys,
                    }),
                );
                self.send(OutKind::SellerPayout, &seller, payout).detach();
                U128(unused)
            }
        }
    }

    /// Push falls back to pull: a failed transfer is credited back (E6).
    #[private]
    pub fn on_transfer_out(&mut self, kind: OutKind, to: AccountId, amount: U128) -> bool {
        let ok = promise_ok();
        self.m.transfer_done(kind.into(), to.as_str(), amount.0, ok);
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
        self.send(OutKind::OwnerWithdraw, to.as_str(), a)
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
        if amount > self.near_withdrawable() {
            env::panic_str("StorageReserve");
        }
        Promise::new(caller).transfer(amount)
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
        self.m
            .set_limits(caller.as_str(), limits.into())
            .unwrap_or_else(|e| fail(e));
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

    pub fn get_earnings(&self) -> core::Earnings {
        self.m.earnings(env::block_timestamp())
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

    pub fn get_sales(&self) -> Vec<Value> {
        self.m.sales.iter().map(|s| self.sale_json(s)).collect()
    }

    pub fn get_claimable(&self, account_id: AccountId) -> U128 {
        U128(*self.m.claimable.get(account_id.as_str()).unwrap_or(&0))
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
            "solvent": self.m.solvent(),
        })
    }

    pub fn near_withdrawable(&self) -> NearToken {
        let storage = env::storage_byte_cost().saturating_mul(env::storage_usage() as u128);
        env::account_balance()
            .saturating_sub(storage)
            .saturating_sub(NEAR_RESERVE)
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
                    .with_static_gas(GAS_NFT_RESOLVE)
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
        let keys = self
            .m
            .transfer_deed(
                receiver_id.as_str(),
                previous_owner_id.as_str(),
                None,
                env::block_timestamp(),
            )
            .unwrap_or_default();
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

fn norm_address(a: &str) -> String {
    let a = a.trim().to_ascii_lowercase();
    let a = a.strip_prefix("0x").unwrap_or(&a).to_string();
    if a.len() != 40 || !a.bytes().all(|c| c.is_ascii_hexdigit()) {
        env::panic_str("BadAddress");
    }
    format!("0x{a}")
}

impl AgentExit {
    fn only_admin(&self) {
        if env::predecessor_account_id() != self.admin {
            env::panic_str("NotAdmin");
        }
    }

    fn owner_id(&self) -> AccountId {
        self.m.owner.parse().expect("valid owner")
    }

    fn send(&mut self, kind: OutKind, to: &str, amount: Amount) -> Promise {
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
                    .with_static_gas(cb_gas(GAS_ON_TRANSFER_OUT))
                    .on_transfer_out(kind, to, U128(amount)),
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
                .with_static_gas(cb_gas(GAS_ON_KEYS))
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
            "sales": self.m.sales.len(),
            "escrow_left": amt(self.m.escrow_left()),
            "frozen": self.m.frozen,
            "known_keys_hash": self.m.known_keys_hash(),
            "limits": {"daily_cap": amt(self.m.limits.daily_cap),
                       "allowed_payees": self.m.limits.allowed_payees.len(),
                       "treasury_bps": self.m.limits.treasury_bps},
            "treasury": amt(self.m.treasury),
            "last_scan": Value::Null,
        });
        Token {
            token_id: DEED_ID.into(),
            owner_id: self.owner_id(),
            metadata: Some(TokenMetadata {
                title: Some(format!("Agent Exit deed: {}", self.m.agent)),
                description: Some(
                    "Ownership deed of an AI agent account. The holder is the owner: sets the runner key and limits, receives the revenue."
                        .into(),
                ),
                media: None,
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
            "terms_min": self.m.terms_min,
            "terms_config": self.m.terms_min.config(),
            "code_hash": l.code_hash,
            "known_keys": self.m.known_keys,
            "known_keys_hash": self.m.known_keys_hash(),
            "token": self.token,
            "listed_at": U64(l.listed_at),
            "treasury_snapshot": amt(l.treasury_snapshot),
        })
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
