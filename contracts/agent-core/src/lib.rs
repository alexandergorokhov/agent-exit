//! Agent Exit core: the pure state machine behind the agent contract.
//!
//! No NEAR runtime calls live here. The contract (`contracts/agent`) passes in
//! the caller, the time and promise outcomes, and turns the returned plans into
//! promises. This keeps every money and key rule testable with plain unit and
//! property tests (invariants I1-I11 in `tests/prop.rs`).
//!
//! Amounts are token base units (the FAKE test dollar has 6 decimals).

pub mod meter;
pub mod money;
pub mod policy;

use borsh::{BorshDeserialize, BorshSerialize};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};

pub use meter::{Earnings, Flags, Meter};
pub use policy::{Limits, Policy, Spent};

pub type Amount = u128;
/// Nanoseconds since the Unix epoch (NEAR block timestamp).
pub type Ts = u64;
pub type Account = String;
/// Public key in NEAR string form, e.g. `ed25519:...`.
pub type Key = String;

/// One dollar of the 6-decimal test token.
pub const USD: Amount = 1_000_000;
pub const NANOS_PER_SEC: u64 = 1_000_000_000;
pub const DAY_NS: u64 = 86_400 * NANOS_PER_SEC;

/// Errors. `Display` prints the bare name, which is the error text the
/// contract panics or logs with, so tests and the break-in script can match it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum Error {
    NotOwner,
    NotRunner,
    NotListed,
    AlreadyListed,
    SaleInProgress,
    ListingChanged,
    KeysChanged,
    Underpaid,
    NotEnoughGas,
    OpenApproval,
    SelfBuy,
    BadTerms,
    BadPrice,
    Frozen,
    Paused,
    InsufficientFunds,
    NothingToClaim,
    UnknownSale,
    WindowOpen,
    WrongSettlement,
    KeyAlreadyKnown,
    SameOwner,
    BadLimits,
    /// A runner payment or incoming payment outside the owner's limits.
    PolicyViolation(Policy),
}

impl core::fmt::Display for Error {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Error::PolicyViolation(p) => write!(f, "PolicyViolation:{p:?}"),
            e => write!(f, "{e:?}"),
        }
    }
}

pub type Res<T> = Result<T, Error>;

/// Earn-out terms. Bounds are enforced by `Terms::check`.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, BorshSerialize, BorshDeserialize,
)]
pub struct Terms {
    /// Part of the price held in escrow (3000 = 30%).
    pub earnout_bps: u16,
    /// Part of each counted post-sale payment released to the seller (5000 = 50%).
    pub share_bps: u16,
    /// Minimum the seller gets from escrow at window end (2000 = 20% of escrow).
    pub floor_bps: u16,
    /// Earn-out window length.
    pub window_secs: u64,
}

impl Default for Terms {
    fn default() -> Self {
        Terms {
            earnout_bps: 3_000,
            share_bps: 5_000,
            floor_bps: 2_000,
            window_secs: 30 * 86_400,
        }
    }
}

impl Terms {
    /// `share_bps <= 50%` is what makes wash-paying lose money (S10).
    pub fn check(&self) -> Res<()> {
        let ok = self.earnout_bps <= 5_000
            && (1..=5_000).contains(&self.share_bps)
            && self.floor_bps <= 10_000
            && (60..=365 * 86_400).contains(&self.window_secs);
        if ok {
            Ok(())
        } else {
            Err(Error::BadTerms)
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, BorshSerialize, BorshDeserialize)]
pub struct Listing {
    pub version: u64,
    pub seller: Account,
    pub price: Amount,
    pub terms: Terms,
    /// Code hash the seller states is deployed. The buyer's scan compares it
    /// with `view_account.code_hash` and the reproducible build hash.
    pub code_hash: String,
    pub listed_at: Ts,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, BorshSerialize, BorshDeserialize)]
pub enum Status {
    /// Owned, not listed, never sold.
    Active,
    Listed,
    /// A buy is in flight: the key batch and its callback have not finished.
    Settling,
    /// Sold at least once and not listed now.
    Sold,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, BorshSerialize, BorshDeserialize)]
pub struct Pending {
    pub sale_id: u64,
    pub buyer: Account,
    pub amount: Amount,
    pub buyer_pk: Key,
    pub listing: Listing,
    pub prev_keys: Vec<Key>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, BorshSerialize, BorshDeserialize)]
pub struct Sale {
    pub id: u64,
    pub seller: Account,
    pub buyer: Account,
    pub price: Amount,
    pub paid_now: Amount,
    pub escrow: Amount,
    pub released: Amount,
    pub returned: Amount,
    pub share_bps: u16,
    pub floor_bps: u16,
    pub started_at: Ts,
    pub window_end: Ts,
    pub finalized: bool,
    /// Set when a backdoor was proven (A23): the rest went back to the buyer.
    pub cut: bool,
}

impl Sale {
    pub fn remaining(&self) -> Amount {
        self.escrow - self.released - self.returned
    }
}

/// What the contract must do after `begin_buy` succeeds.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BuyPlan {
    pub sale_id: u64,
    /// Runner keys to delete (the seller's).
    pub delete_keys: Vec<Key>,
    /// Runner key to add (the buyer's, function-call only).
    pub add_key: Key,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BuyResult {
    /// Handover done. Pay `payout` to `seller`; return `unused` to the buyer
    /// through the token's `ft_resolve_transfer`.
    Done {
        sale: Box<Sale>,
        seller: Account,
        payout: Amount,
        unused: Amount,
    },
    /// Key batch failed: everything is as before; `unused` (the full amount)
    /// goes back to the buyer.
    RolledBack { unused: Amount },
}

/// Why a token transfer leaves the contract; decides where a failed one is
/// credited back.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum TransferKind {
    SellerPayout,
    Claim,
    OwnerWithdraw,
    /// The runner paid a payee out of the treasury (feature 12).
    TreasuryPay,
}

/// Outcome of a payment routed through `ft_on_transfer` with kind `pay`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PayResult {
    Refund(&'static str),
    Recorded {
        flags: Flags,
        counted: bool,
        verified: bool,
        released: Vec<(u64, Account, Amount)>,
        /// The automatic split of this payment (feature 12).
        to_owner: Amount,
        to_treasury: Amount,
    },
}

/// Result of a runner-method guard.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RunnerCheck {
    Ok,
    /// A self-call was signed by a key the contract never added (A23). The
    /// agent is now frozen and the open escrow went back to buyers.
    BackdoorProven {
        returned: Vec<(u64, Account, Amount)>,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, BorshSerialize, BorshDeserialize, Serialize, Deserialize)]
pub struct Machine {
    pub agent: Account,
    pub owner: Account,
    pub owner_epoch: u64,
    /// Every account that owned the deed before (their payments are `self`).
    pub past_owners: BTreeSet<Account>,
    pub status: Status,
    /// Status to go back to if a listing is cancelled.
    pub status_before_listing: Status,
    pub listing: Option<Listing>,
    /// Bumped on every listing change (list, cancel, transfer, sale).
    pub listing_version: u64,
    pub pending: Option<Pending>,
    pub known_keys: BTreeSet<Key>,
    pub paused: bool,
    pub frozen: bool,
    pub approvals: BTreeMap<Account, u64>,
    pub next_approval_id: u64,
    pub meter: Meter,
    pub sales: Vec<Sale>,
    pub next_sale_id: u64,
    pub claimable: BTreeMap<Account, Amount>,
    /// Owner's free token balance (revenue).
    pub owner_free: Amount,
    /// Tokens the contract believes it holds (excludes transfers in flight).
    pub held: Amount,
    /// Accounts the agent sent tokens to (for the `round_trip` flag), bounded.
    pub paid_to: BTreeSet<Account>,
    /// Latest `valid_before` of any Base authorization the agent signed (B3).
    pub open_auth_until: Ts,
    /// Owner's limits (feature 12); they move with the deed.
    pub limits: Limits,
    /// The agent's own funds; only the runner spends them, inside the limits.
    pub treasury: Amount,
    /// Runner spending in the current (owner epoch, day).
    pub spent: Spent,
}

pub const MAX_PAID_TO: usize = 1_000;

/// Hash of a key set as the buyer saw it: sha256 over the sorted keys joined
/// by `\n`, hex encoded.
pub fn keys_hash<'a>(keys: impl IntoIterator<Item = &'a Key>) -> String {
    let mut v: Vec<&Key> = keys.into_iter().collect();
    v.sort();
    let joined = v.iter().map(|k| k.as_str()).collect::<Vec<_>>().join("\n");
    hex::encode(Sha256::digest(joined.as_bytes()))
}

impl Machine {
    pub fn new(agent: &str, owner: &str) -> Self {
        Machine {
            agent: agent.into(),
            owner: owner.into(),
            owner_epoch: 1,
            past_owners: BTreeSet::new(),
            status: Status::Active,
            status_before_listing: Status::Active,
            listing: None,
            listing_version: 0,
            pending: None,
            known_keys: BTreeSet::new(),
            paused: true,
            frozen: false,
            approvals: BTreeMap::new(),
            next_approval_id: 0,
            meter: Meter::default(),
            sales: Vec::new(),
            next_sale_id: 1,
            claimable: BTreeMap::new(),
            owner_free: 0,
            held: 0,
            paid_to: BTreeSet::new(),
            open_auth_until: 0,
            limits: Limits::default(),
            treasury: 0,
            spent: Spent::default(),
        }
    }

    pub fn known_keys_hash(&self) -> String {
        keys_hash(self.known_keys.iter())
    }

    fn only_owner(&self, caller: &str) -> Res<()> {
        if caller == self.owner {
            Ok(())
        } else {
            Err(Error::NotOwner)
        }
    }

    fn not_settling(&self) -> Res<()> {
        if self.status == Status::Settling {
            Err(Error::SaleInProgress)
        } else {
            Ok(())
        }
    }

    /// Escrow still owed across all sales.
    pub fn escrow_left(&self) -> Amount {
        self.sales.iter().map(Sale::remaining).sum()
    }

    pub fn claimable_total(&self) -> Amount {
        self.claimable.values().sum()
    }

    fn credit(&mut self, who: &str, amount: Amount) {
        if amount > 0 {
            *self.claimable.entry(who.to_string()).or_default() += amount;
        }
    }

    // ----- keys -------------------------------------------------------------

    /// Owner replaces the runner key. Returns the keys to delete; the contract
    /// batches the `delete_key`s and `add_key(new)` on itself.
    pub fn set_runner_key(&mut self, caller: &str, new_key: &str) -> Res<Vec<Key>> {
        self.only_owner(caller)?;
        self.not_settling()?;
        if self.frozen {
            return Err(Error::Frozen);
        }
        if self.status == Status::Listed {
            // Changing keys while listed would invalidate what buyers scanned.
            return Err(Error::AlreadyListed);
        }
        if self.known_keys.contains(new_key) {
            return Err(Error::KeyAlreadyKnown);
        }
        let old: Vec<Key> = self.known_keys.iter().cloned().collect();
        self.known_keys.clear();
        self.known_keys.insert(new_key.to_string());
        self.paused = false;
        Ok(old)
    }

    /// The key batch from `set_runner_key` failed: restore the old key set.
    pub fn runner_key_failed(&mut self, old: Vec<Key>, new_key: &str) {
        self.known_keys.remove(new_key);
        self.known_keys.extend(old);
        self.paused = self.known_keys.is_empty();
    }

    // ----- deed approvals (NEP-178) -----------------------------------------

    pub fn approve(&mut self, caller: &str, account: &str) -> Res<u64> {
        self.only_owner(caller)?;
        self.not_settling()?;
        if self.status == Status::Listed {
            // An approval would let someone move the deed outside buy().
            return Err(Error::OpenApproval);
        }
        let id = self.next_approval_id;
        self.next_approval_id += 1;
        self.approvals.insert(account.to_string(), id);
        Ok(id)
    }

    pub fn revoke(&mut self, caller: &str, account: Option<&str>) -> Res<()> {
        self.only_owner(caller)?;
        match account {
            Some(a) => {
                self.approvals.remove(a);
            }
            None => self.approvals.clear(),
        }
        Ok(())
    }

    // ----- listing ----------------------------------------------------------

    pub fn list(
        &mut self,
        caller: &str,
        price: Amount,
        terms: Terms,
        code_hash: &str,
        now: Ts,
    ) -> Res<Listing> {
        self.only_owner(caller)?;
        self.not_settling()?;
        if self.frozen {
            return Err(Error::Frozen);
        }
        if self.status == Status::Listed {
            return Err(Error::AlreadyListed);
        }
        if !self.approvals.is_empty() || self.open_auth_until > now {
            return Err(Error::OpenApproval);
        }
        if price < USD {
            return Err(Error::BadPrice);
        }
        terms.check()?;
        self.listing_version += 1;
        self.status_before_listing = self.status.clone();
        self.status = Status::Listed;
        let l = Listing {
            version: self.listing_version,
            seller: self.owner.clone(),
            price,
            terms,
            code_hash: code_hash.to_string(),
            listed_at: now,
        };
        self.listing = Some(l.clone());
        Ok(l)
    }

    pub fn cancel_listing(&mut self, caller: &str) -> Res<()> {
        self.only_owner(caller)?;
        self.not_settling()?;
        if self.status != Status::Listed {
            return Err(Error::NotListed);
        }
        self.listing = None;
        self.listing_version += 1;
        self.status = self.status_before_listing.clone();
        Ok(())
    }

    // ----- buy / handover ---------------------------------------------------

    /// Validate a buy and lock the agent in `Settling`. Nothing changes on
    /// error, and the caller refunds the full amount. `gas_ok` is the
    /// contract's up-front check that the callback can run (E7).
    #[allow(clippy::too_many_arguments)]
    pub fn begin_buy(
        &mut self,
        buyer: &str,
        amount: Amount,
        listing_version: u64,
        known_keys_hash: &str,
        buyer_pk: &str,
        gas_ok: bool,
        now: Ts,
    ) -> Res<BuyPlan> {
        if self.status == Status::Settling {
            return Err(Error::SaleInProgress);
        }
        if self.frozen {
            return Err(Error::Frozen);
        }
        let listing = match (&self.status, &self.listing) {
            (Status::Listed, Some(l)) => l.clone(),
            _ => return Err(Error::NotListed),
        };
        if listing_version != listing.version {
            return Err(Error::ListingChanged);
        }
        if known_keys_hash != self.known_keys_hash() {
            return Err(Error::KeysChanged);
        }
        if buyer == self.owner {
            return Err(Error::SelfBuy);
        }
        if amount < listing.price {
            return Err(Error::Underpaid);
        }
        if self.open_auth_until > now || !self.approvals.is_empty() {
            return Err(Error::OpenApproval);
        }
        if !gas_ok {
            return Err(Error::NotEnoughGas);
        }
        let sale_id = self.next_sale_id;
        self.next_sale_id += 1;
        let prev_keys: Vec<Key> = self.known_keys.iter().cloned().collect();
        self.held += amount;
        self.status = Status::Settling;
        self.pending = Some(Pending {
            sale_id,
            buyer: buyer.to_string(),
            amount,
            buyer_pk: buyer_pk.to_string(),
            listing,
            prev_keys: prev_keys.clone(),
        });
        Ok(BuyPlan {
            sale_id,
            delete_keys: prev_keys,
            add_key: buyer_pk.to_string(),
        })
    }

    /// Callback after the self key batch. `keys_ok` is the batch outcome.
    pub fn finish_buy(&mut self, sale_id: u64, keys_ok: bool, now: Ts) -> Res<BuyResult> {
        let p = match &self.pending {
            Some(p) if p.sale_id == sale_id && self.status == Status::Settling => p.clone(),
            _ => return Err(Error::WrongSettlement),
        };
        self.pending = None;
        if !keys_ok {
            // Roll back: listing, keys and owner were never changed.
            self.status = Status::Listed;
            self.held -= p.amount;
            return Ok(BuyResult::RolledBack { unused: p.amount });
        }
        let l = &p.listing;
        let (paid_now, escrow) = money::split_price(l.price, l.terms.earnout_bps);
        let unused = p.amount - l.price;
        // Deed moves, owner epoch bumps, seller keys are gone.
        self.past_owners.insert(self.owner.clone());
        self.owner = p.buyer.clone();
        self.owner_epoch += 1;
        self.approvals.clear();
        self.listing = None;
        self.listing_version += 1;
        self.status = Status::Sold;
        self.known_keys.clear();
        self.known_keys.insert(p.buyer_pk.clone());
        self.paused = false;
        let sale = Sale {
            id: sale_id,
            seller: l.seller.clone(),
            buyer: p.buyer.clone(),
            price: l.price,
            paid_now,
            escrow,
            released: 0,
            returned: 0,
            share_bps: l.terms.share_bps,
            floor_bps: l.terms.floor_bps,
            started_at: now,
            window_end: now + l.terms.window_secs * NANOS_PER_SEC,
            finalized: false,
            cut: false,
        };
        let mut sale = sale;
        if self.frozen {
            // A hidden key was proven while settling: the buyer gets the escrow back.
            sale.returned = sale.escrow;
            sale.finalized = true;
            sale.cut = true;
            self.credit(&p.buyer, sale.escrow);
        }
        self.sales.push(sale.clone());
        // The overpay goes back through ft_resolve_transfer; the payout leaves now.
        self.held -= unused + paid_now;
        self.note_paid_to(&l.seller);
        Ok(BuyResult::Done {
            sale: Box::new(sale),
            seller: l.seller.clone(),
            payout: paid_now,
            unused,
        })
    }

    // ----- token transfers out ----------------------------------------------

    /// A transfer started by the contract finished. On failure the amount is
    /// credited back (push falls back to pull, E6).
    pub fn transfer_done(&mut self, kind: TransferKind, to: &str, amount: Amount, ok: bool) {
        if ok {
            return;
        }
        self.held += amount;
        match kind {
            TransferKind::SellerPayout | TransferKind::Claim => self.credit(to, amount),
            TransferKind::OwnerWithdraw => self.owner_free += amount,
            // A failed supplier payment goes back to the treasury. The day's
            // spending is not reduced (the cap counts attempts, conservative).
            TransferKind::TreasuryPay => self.treasury += amount,
        }
    }

    /// Anyone with a claimable balance pulls it.
    pub fn claim(&mut self, caller: &str) -> Res<Amount> {
        let amount = self.claimable.remove(caller).unwrap_or(0);
        if amount == 0 {
            return Err(Error::NothingToClaim);
        }
        self.held -= amount;
        Ok(amount)
    }

    /// Owner takes revenue out, to themselves or another account.
    pub fn withdraw(&mut self, caller: &str, amount: Amount, to: &str) -> Res<Amount> {
        self.only_owner(caller)?;
        self.not_settling()?;
        if amount == 0 || amount > self.owner_free {
            return Err(Error::InsufficientFunds);
        }
        self.owner_free -= amount;
        self.held -= amount;
        self.note_paid_to(to);
        Ok(amount)
    }

    fn note_paid_to(&mut self, to: &str) {
        if self.paid_to.len() < MAX_PAID_TO {
            self.paid_to.insert(to.to_string());
        }
    }

    // ----- payments and earn-out --------------------------------------------

    /// Classify a payment by who paid.
    pub fn flags_for(&self, payer: &str) -> Flags {
        let is_self =
            payer == self.agent || payer == self.owner || self.past_owners.contains(payer);
        let mut parents = std::iter::once(&self.agent)
            .chain(std::iter::once(&self.owner))
            .chain(self.past_owners.iter());
        let sub = parents.any(|p| payer.len() > p.len() + 1 && payer.ends_with(&format!(".{p}")));
        Flags {
            self_pay: is_self,
            seller_subaccount: !is_self && sub,
            round_trip: !is_self && !sub && self.paid_to.contains(payer),
        }
    }

    /// Record an incoming payment. `verified` is the contract's NEAR AI
    /// signature check; it only counts for a counted payment (I9).
    pub fn pay(&mut self, payer: &str, amount: Amount, verified: bool, now: Ts) -> PayResult {
        if amount < meter::MIN_PAYMENT {
            return PayResult::Refund("Dust");
        }
        if amount < self.limits.price_floor {
            return PayResult::Refund("PolicyViolation:PriceFloor");
        }
        let flags = self.flags_for(payer);
        let counted = !flags.any();
        let verified = counted && verified;
        self.meter.record(payer, amount, flags, verified, now);
        self.held += amount;
        let (to_owner, to_treasury) = money::split_revenue(amount, self.limits.treasury_bps);
        self.owner_free += to_owner;
        self.treasury += to_treasury;
        let mut released = Vec::new();
        if counted {
            // Release from the oldest open earn-out, at most share_bps of this
            // payment in total (S10: wash-paying always loses money).
            if let Some(sale) = self
                .sales
                .iter_mut()
                .find(|s| !s.finalized && s.remaining() > 0 && now < s.window_end)
            {
                let r = money::release_for_payment(amount, sale.share_bps, sale.remaining());
                if r > 0 {
                    sale.released += r;
                    released.push((sale.id, sale.seller.clone(), r));
                }
            }
            for (_, seller, r) in &released {
                self.credit(seller, *r);
            }
        }
        PayResult::Recorded {
            flags,
            counted,
            verified,
            released,
            to_owner,
            to_treasury,
        }
    }

    // ----- spending policy (feature 12) ---------------------------------------

    /// Owner sets the limits. Refused while listed (buyers scanned them) or
    /// settling; refused `BadLimits` if they fail the sanity checks (E33).
    pub fn set_limits(&mut self, caller: &str, limits: Limits) -> Res<()> {
        self.only_owner(caller)?;
        self.not_settling()?;
        if self.frozen {
            return Err(Error::Frozen);
        }
        if self.status == Status::Listed {
            return Err(Error::AlreadyListed);
        }
        limits.check(&self.agent)?;
        self.limits = limits;
        Ok(())
    }

    pub fn spent_today(&self, now: Ts) -> Amount {
        self.spent.today(self.owner_epoch, now / DAY_NS)
    }

    /// The runner pays `payee` out of the treasury. Call only after
    /// `runner_guard` returned `Ok`. Checks, in order: payee allowed, daily
    /// cap, treasury balance. Nothing changes on error.
    pub fn spend(&mut self, payee: &str, amount: Amount, now: Ts) -> Res<Amount> {
        if amount == 0 {
            return Err(Error::InsufficientFunds);
        }
        if !self.limits.allowed_payees.contains(payee) {
            return Err(Error::PolicyViolation(Policy::Payee));
        }
        let day = now / DAY_NS;
        let spent = self.spent.today(self.owner_epoch, day);
        match spent.checked_add(amount) {
            Some(total) if total <= self.limits.daily_cap => {}
            _ => return Err(Error::PolicyViolation(Policy::DailyCap)),
        }
        if amount > self.treasury {
            return Err(Error::InsufficientFunds);
        }
        self.treasury -= amount;
        self.held -= amount;
        self.spent = Spent {
            owner_epoch: self.owner_epoch,
            day,
            amount: spent + amount,
        };
        self.note_paid_to(payee);
        Ok(spent + amount)
    }

    /// Close an earn-out after its window. Anyone may call; a second call is a
    /// no-op that returns the same `Sale` (E21).
    pub fn finalize_earnout(&mut self, sale_id: u64, now: Ts) -> Res<Sale> {
        let idx = self
            .sales
            .iter()
            .position(|s| s.id == sale_id)
            .ok_or(Error::UnknownSale)?;
        if self.sales[idx].finalized {
            return Ok(self.sales[idx].clone());
        }
        if now < self.sales[idx].window_end {
            return Err(Error::WindowOpen);
        }
        let s = &mut self.sales[idx];
        let f = money::finalize(s.escrow, s.released + s.returned, s.floor_bps);
        s.released += f.seller_topup;
        s.returned += f.buyer_refund;
        s.finalized = true;
        let s = s.clone();
        self.credit(&s.seller, f.seller_topup);
        self.credit(&s.buyer, f.buyer_refund);
        Ok(s)
    }

    // ----- deed transfer outside buy() ---------------------------------------

    /// `nft_transfer` by the owner or an approved account. Runs the same
    /// handover as a sale minus the money: all runner keys are deleted and the
    /// agent pauses until the new owner sets a runner key (E24, A16).
    pub fn transfer_deed(
        &mut self,
        caller: &str,
        receiver: &str,
        approval_id: Option<u64>,
    ) -> Res<Vec<Key>> {
        self.not_settling()?;
        if caller != self.owner {
            match (self.approvals.get(caller), approval_id) {
                (Some(id), Some(given)) if *id == given => {}
                (Some(_), None) => {}
                _ => return Err(Error::NotOwner),
            }
        }
        if receiver == self.owner {
            return Err(Error::SameOwner);
        }
        self.approvals.clear();
        if self.status == Status::Listed {
            self.listing = None;
            self.status = self.status_before_listing.clone();
        }
        self.listing_version += 1;
        self.past_owners.insert(self.owner.clone());
        self.owner = receiver.to_string();
        self.owner_epoch += 1;
        let keys: Vec<Key> = self.known_keys.iter().cloned().collect();
        self.known_keys.clear();
        self.paused = true;
        Ok(keys)
    }

    // ----- runner guard ------------------------------------------------------

    /// Every runner method calls this first. Runner methods must be self-calls
    /// signed by a key this contract added. A self-call signed by any other key
    /// proves a hidden key: freeze and return open escrow to buyers (A23).
    pub fn runner_guard(&mut self, predecessor: &str, signer_pk: &str) -> Res<RunnerCheck> {
        if predecessor != self.agent {
            return Err(Error::NotRunner);
        }
        if self.frozen {
            return Err(Error::Frozen);
        }
        if !self.known_keys.contains(signer_pk) {
            self.frozen = true;
            let mut returned = Vec::new();
            for s in self.sales.iter_mut().filter(|s| !s.finalized) {
                let r = s.remaining();
                s.returned += r;
                s.finalized = true;
                s.cut = true;
                returned.push((s.id, s.buyer.clone(), r));
            }
            for (_, buyer, r) in &returned {
                self.credit(buyer, *r);
            }
            return Ok(RunnerCheck::BackdoorProven { returned });
        }
        self.not_settling()?;
        if self.paused {
            return Err(Error::Paused);
        }
        Ok(RunnerCheck::Ok)
    }

    /// Solvency (I3): what the contract holds covers every bucket.
    pub fn solvent(&self) -> bool {
        self.held >= self.escrow_left() + self.claimable_total() + self.owner_free + self.treasury
    }

    pub fn earnings(&self, now: Ts) -> Earnings {
        self.meter.earnings(now)
    }
}
