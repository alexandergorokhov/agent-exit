//! Agent Exit core: the pure state machine behind the agent contract.
//!
//! No NEAR runtime calls live here. The contract (`contracts/agent`) passes in
//! the caller, the time and promise outcomes, and turns the returned plans into
//! promises. This keeps every money and key rule testable with plain unit and
//! property tests (invariants I1-I11 in `tests/prop.rs`).
//!
//! Amounts are token base units (the FAKE test dollar has 6 decimals).

pub mod base;
pub mod meter;
pub mod money;
pub mod policy;
pub mod sealed;
pub mod shares;

use borsh::{BorshDeserialize, BorshSerialize};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};

/// Former name of `Store`.
pub use meter::Store as MeterStore;
pub use meter::{DayStats, Earnings, Flags, MemStore, Meter, Store};
pub use policy::{Limits, Policy, Spent};
pub use shares::{Holding, ShareOffer, ShareTerms};

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
    /// `MAX_OPEN_SALES` earn-outs are open: finalize one before listing (A47).
    TooManyOpenSales,
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
    /// The runner tried to pay while the agent is listed (A24).
    SpendingPaused,
    /// The treasury is below what the listing showed (A24, stale listing).
    TreasuryChanged,
    /// A key add or delete is still waiting for its callback (A25).
    KeysPending,
    /// `paid_to` is full: send only to accounts already paid or to the owner (A30).
    PaidToFull,
    /// Recovery asked for, but nothing is stuck (A33).
    NothingToRecover,
    /// Recovery asked for before `RECOVER_AFTER_NS` passed: the callback may
    /// still run (A33).
    RecoverTooEarly,
    /// `pay_base` on an agent deployed without a Base wallet address.
    BaseWalletNotSet,
    /// A Base authorization valid for more than an hour (E23).
    ValidityTooLong,
    /// A runner payment or incoming payment outside the owner's limits.
    PolicyViolation(Policy),
    /// `MAX_PENDING_AUTHS` Base authorizations are waiting for the MPC
    /// signer; wait for a callback before asking again (STR-19).
    TooManyPendingAuths,
    /// Open share slices would exceed `shares::MAX_SHARES_BPS` (SH1).
    SharesTooHigh,
    /// Share slice terms out of bounds (bps, price or end).
    BadShareTerms,
    UnknownOffer,
    /// The slice is not what the investor saw (T6).
    ShareChanged,
    /// The slice ended or is sold out.
    OfferClosed,
    /// `shares::MAX_HOLDERS` investors already hold shares (SH5).
    TooManyHolders,
    /// Below the smallest part (T10).
    ShareTooSmall,
    /// Investors hold shares: this change would cut their earnings (T4, T7).
    SharesOpen,
    /// `shares::MAX_OPEN_OFFERS` slices are open: close one first.
    TooManyOffers,
    // ----- sealed agent config (sealed.rs, SC1-SC12) -----
    /// The listing sells a sealed config: the buy must carry a one-time
    /// X25519 delivery key.
    DeliveryKeyRequired,
    /// A commitment that is not 64 lowercase hex chars and 1..=8192 bytes.
    BadConfig,
    /// The sale has no sealed config.
    NoSealedConfig,
    NotSeller,
    NotBuyer,
    AlreadyDelivered,
    /// The delivery deadline passed.
    DeliveryLate,
    /// The deadline has not passed yet.
    DeliveryOpen,
    /// Not delivered yet, or the dispute window closed.
    DisputeClosed,
    /// SC14: only the buyer may accept before the dispute window closes.
    DisputeOpen,
    /// Review 2: the deed cannot be sent to the agent's own account.
    DeedToAgent,
    /// The secret key is not the one the buy named.
    BadDeliveryKey,
    /// The delivery opens to exactly the committed bundle: no dispute.
    ConfigMatches,
}

impl core::fmt::Display for Error {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Error::PolicyViolation(p) => write!(f, "PolicyViolation:{p:?}"),
            Error::SpendingPaused => write!(f, "Listed: spending paused"),
            e => write!(f, "{e:?}"),
        }
    }
}

pub type Res<T> = Result<T, Error>;

/// Earn-out terms. Bounds are enforced by `Terms::check`.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, BorshSerialize, BorshDeserialize,
)]
#[cfg_attr(
    not(target_arch = "wasm32"),
    derive(borsh::BorshSchema, schemars::JsonSchema)
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

/// The whole price may be held: the seller is then paid only from revenue.
pub const MAX_EARNOUT_BPS: u16 = 10_000;

impl Terms {
    /// "Pay from revenue": nothing is paid to the seller at the sale.
    pub fn pay_from_revenue(&self) -> bool {
        self.earnout_bps == MAX_EARNOUT_BPS
    }

    /// What a seller must pay in through other wallets, after the sale, to
    /// pull `released` out of the escrow: `released / share_bps`. They lose
    /// the difference, so faking revenue after a sale never pays (S10).
    pub fn cost_to_fake_release(&self, released: Amount) -> Amount {
        if self.share_bps == 0 {
            return Amount::MAX;
        }
        released.saturating_mul(money::BPS) / self.share_bps as Amount
    }
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

/// The least protection a listing must give the buyer (A32). The contract
/// refuses a listing below it and the buyer's scan blocks one, so the 30%
/// earn-out does not depend on the seller's choice.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, BorshSerialize, BorshDeserialize,
)]
#[cfg_attr(
    not(target_arch = "wasm32"),
    derive(borsh::BorshSchema, schemars::JsonSchema)
)]
pub struct TermsMin {
    /// Least part of the price held in escrow.
    pub min_earnout_bps: u16,
    /// Most of the escrow the seller is guaranteed at window end.
    pub max_floor_bps: u16,
    /// Shortest earn-out window.
    pub min_window_secs: u64,
}

impl TermsMin {
    /// The published minimum: 30% escrow, floor at most 50% of it, 30 days.
    pub const PROD: TermsMin = TermsMin {
        min_earnout_bps: 3_000,
        max_floor_bps: 5_000,
        min_window_secs: 30 * 86_400,
    };

    /// `prod` when the window minimum is the published 30 days, else `demo`
    /// (a demo build may shorten the window; every view labels it).
    pub fn config(&self) -> &'static str {
        if self.min_window_secs >= Self::PROD.min_window_secs {
            "prod"
        } else {
            "demo"
        }
    }
}

impl Default for TermsMin {
    fn default() -> Self {
        Self::PROD
    }
}

impl Terms {
    /// Shape and buyer minimum. `share_bps <= 50%` is what makes wash-paying
    /// lose money (S10); `min` is the buyer protection (A32). The held part
    /// can be the whole price (`earnout_bps` 10000, "pay from revenue", PR1):
    /// then the seller is paid only by customer payments after the sale.
    pub fn check(&self, min: &TermsMin) -> Res<()> {
        let ok = (min.min_earnout_bps.max(1)..=MAX_EARNOUT_BPS).contains(&self.earnout_bps)
            && (1..=5_000).contains(&self.share_bps)
            && self.floor_bps <= min.max_floor_bps.min(10_000)
            && (min.min_window_secs.max(60)..=365 * 86_400).contains(&self.window_secs);
        if ok {
            Ok(())
        } else {
            Err(Error::BadTerms)
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, BorshSerialize, BorshDeserialize)]
#[cfg_attr(not(target_arch = "wasm32"), derive(borsh::BorshSchema))]
pub struct Listing {
    pub version: u64,
    pub seller: Account,
    pub price: Amount,
    pub terms: Terms,
    /// Code hash the seller states is deployed. The buyer's scan compares it
    /// with `view_account.code_hash` and the reproducible build hash.
    pub code_hash: String,
    pub listed_at: Ts,
    /// Treasury balance when listed. Spending is paused while listed, and
    /// `buy` refuses if the treasury is ever below this (A24).
    pub treasury_snapshot: Amount,
    /// The sealed agent config this sale delivers (sealed.rs), if any.
    #[serde(default)]
    pub config: Option<sealed::ConfigCommit>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, BorshSerialize, BorshDeserialize)]
#[cfg_attr(not(target_arch = "wasm32"), derive(borsh::BorshSchema))]
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
#[cfg_attr(not(target_arch = "wasm32"), derive(borsh::BorshSchema))]
pub struct Pending {
    pub sale_id: u64,
    pub buyer: Account,
    pub amount: Amount,
    pub buyer_pk: Key,
    pub listing: Listing,
    pub prev_keys: Vec<Key>,
    /// When the buy started; `recover_sale` is allowed `RECOVER_AFTER_NS` later.
    pub started_at: Ts,
    /// The buyer's one-time X25519 key for a sealed config (hex).
    #[serde(default)]
    pub delivery_pk: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, BorshSerialize, BorshDeserialize)]
#[cfg_attr(not(target_arch = "wasm32"), derive(borsh::BorshSchema))]
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
    /// The seller's owner epoch. Only accounts the agent paid in this epoch
    /// or earlier count as round trips for this earn-out: the buyer cannot
    /// stop releases by paying customers after the sale (A31).
    pub seller_epoch: u64,
    pub started_at: Ts,
    pub window_end: Ts,
    pub finalized: bool,
    /// Set when a backdoor was proven (A23): the rest went back to the buyer.
    pub cut: bool,
    /// Payout destinations ever recorded up to the end of `seller_epoch`
    /// (`Machine::paid_to_ever` at the sale). Pruning up to this sale's
    /// era drops exactly that many (A53).
    #[serde(default)]
    pub paid_to_cum: u64,
    /// The sealed config's delivery (sealed.rs): releases wait for it.
    #[serde(default)]
    pub config: Option<sealed::SaleConfig>,
}

impl Sale {
    pub fn remaining(&self) -> Amount {
        self.escrow - self.released - self.returned
    }

    /// Everything the buyer got back: the escrow returned plus, for a sealed
    /// sale whose delivery was missed or disproved, the held upfront part.
    pub fn back_to_buyer(&self) -> Amount {
        self.returned + self.config.as_ref().map_or(0, |c| c.upfront_returned)
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
        /// Protocol fee credited to `fee_to` (claimable), out of the upfront part.
        fee: Amount,
        unused: Amount,
        /// The seller's unwithdrawn revenue, credited to the seller (A29).
        seller_revenue: Amount,
    },
    /// Key batch failed: everything is as before; `unused` (the full amount)
    /// goes back to the buyer. Or a backdoor was proven while settling
    /// (A34): the buyer is refunded in full, the seller keeps the frozen
    /// agent, the listing is withdrawn and `delete_keys` (the buyer's key,
    /// if the batch added it) are deleted.
    RolledBack {
        unused: Amount,
        frozen: bool,
        delete_keys: Vec<Key>,
    },
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
        /// Investors' part of this payment, credited to them (SH2, T1).
        to_investors: Amount,
        /// Price escrow released by this payment, per slice seller (SH11).
        share_released: Vec<(Account, Amount)>,
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
#[cfg_attr(not(target_arch = "wasm32"), derive(borsh::BorshSchema))]
pub struct Machine<S = MemStore> {
    pub agent: Account,
    pub owner: Account,
    pub owner_epoch: u64,
    /// How many accounts owned the deed before (each in `store`; their
    /// payments are `self`).
    pub past_owners: u64,
    pub status: Status,
    /// Status to go back to if a listing is cancelled.
    pub status_before_listing: Status,
    pub listing: Option<Listing>,
    /// Bumped on every listing change (list, cancel, transfer, sale).
    pub listing_version: u64,
    pub pending: Option<Pending>,
    pub known_keys: BTreeSet<Key>,
    /// Keys being added by a key batch whose callback has not run yet (the
    /// buyer's key during `buy`, the new key during a rotation). A signature
    /// by one of them is `NotRunner`, never a backdoor (A25).
    pub incoming: BTreeSet<Key>,
    /// Keys being deleted whose deletion is not confirmed yet. Same rule.
    pub retiring: BTreeSet<Key>,
    pub paused: bool,
    pub frozen: bool,
    pub approvals: BTreeMap<Account, u64>,
    pub next_approval_id: u64,
    pub meter: Meter,
    /// Open earn-outs (at most `MAX_OPEN_SALES`). Finalized sales move to
    /// the sale history in `store` (A47).
    pub sales: Vec<Sale>,
    /// Sales in the history.
    pub closed_sales: u64,
    /// Highest seller epoch of a finalized sale (paid_to pruning, A45).
    pub closed_max_epoch: Option<u64>,
    /// `paid_to_cum` of that sale (A53).
    pub closed_max_cum: u64,
    pub next_sale_id: u64,
    /// Sum of the claimable balances (each in `store`).
    pub claimable_sum: Amount,
    /// Accounts with a claimable balance.
    pub claimable_accounts: u64,
    /// Owner's free token balance (revenue).
    pub owner_free: Amount,
    /// Tokens the contract believes it holds (excludes transfers in flight).
    pub held: Amount,
    /// Protocol fee account (claimable balance); empty = no fee.
    pub fee_to: Account,
    /// Accounts the agent sent tokens to (for the `round_trip` flag), with
    /// the owner epoch of the first send. Bounded (A30). Entries live in
    /// `store`; here only counters, so the blob does not grow with owner
    /// epochs (A53): entries ever recorded, entries of the current epoch,
    /// and the pruned prefix (epochs `<= paid_to_dead_upto`, A45) with its
    /// entry count. A stored entry of a pruned epoch counts as absent.
    pub paid_to_ever: u64,
    pub paid_to_this_epoch: u64,
    pub paid_to_dead_upto: u64,
    pub paid_to_dead_count: u64,
    /// Latest `valid_before` of any Base authorization the agent signed or
    /// asked the MPC signer to sign (feature 5); list and buy are refused
    /// until it passes (A9).
    pub open_auth_until: Ts,
    /// The agent's Base Sepolia wallet (feature 5).
    pub base: base::BaseWallet,
    /// Owner's limits (feature 12); they move with the deed.
    pub limits: Limits,
    /// The agent's own funds; only the runner spends them, inside the limits.
    pub treasury: Amount,
    /// Runner spending in the current (owner epoch, day).
    pub spent: Spent,
    /// Least terms a listing must give the buyer (A32), fixed at init.
    pub terms_min: TermsMin,
    /// When the last key batch started (rotation, deed transfer, recovery);
    /// `recover_keys` is allowed `RECOVER_AFTER_NS` later (A33).
    pub keys_since: Ts,
    /// Nonce of the latest rotation batch. Every rotation and every
    /// `recover_*` bumps it; a rotation callback that carries an older nonce
    /// is stale and changes no key set (A44).
    pub key_batch: u64,
    /// The last buy `recover_sale` undid. A late `on_handover` for it is
    /// stale: it refunds the buyer and deletes the buyer key its batch may
    /// still have added (A44).
    pub recovered_buy: Option<Pending>,
    /// Shares of future earnings (PLAN-shares.md): open slices.
    #[serde(default)]
    pub offers: Vec<ShareOffer>,
    #[serde(default)]
    pub next_offer_id: u64,
    /// Price escrow held for investors across all slices (SH11).
    #[serde(default)]
    pub share_escrow: Amount,
    /// The owner's commitment to the sealed config the next listing sells.
    #[serde(default)]
    pub config_commit: Option<sealed::ConfigCommit>,
    /// Per-payer and per-day rows (A47). In the contract this is a handle to
    /// separate storage keys and adds nothing to the state blob.
    #[serde(skip)]
    pub store: S,
}

/// How long a callback gets before recovery is allowed (A33). A callback
/// runs within a few blocks or has failed; an hour is far past both.
pub const RECOVER_AFTER_NS: u64 = 3_600 * NANOS_PER_SEC;

/// Keys to delete after a recovery, each in its own receipt.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Recovery {
    pub delete_keys: Vec<Key>,
    /// The buy that was rolled back (`recover_sale` only).
    pub sale: Option<Pending>,
}

/// New payout destinations each owner may add (A30). Counted per owner
/// epoch, so a seller who filled the table cannot leave the buyer with
/// `PaidToFull` (A36).
pub const MAX_PAID_TO: usize = 1_000;
/// Hard cap over all owners. `get_status.paid_to_total` shows the size and
/// the scan blocks an agent whose buyer would not get a full quota (A36).
pub const MAX_PAID_TO_TOTAL: usize = 3_000;
/// Open earn-outs at a time; each needs a real sale, and listing waits
/// until one is finalized (A47).
pub const MAX_OPEN_SALES: usize = 16;

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
        Machine::with_store(agent, owner)
    }
}

impl<S: Store + Default> Machine<S> {
    pub fn with_store(agent: &str, owner: &str) -> Self {
        Machine {
            agent: agent.into(),
            owner: owner.into(),
            owner_epoch: 1,
            past_owners: 0,
            status: Status::Active,
            status_before_listing: Status::Active,
            listing: None,
            listing_version: 0,
            pending: None,
            known_keys: BTreeSet::new(),
            incoming: BTreeSet::new(),
            retiring: BTreeSet::new(),
            paused: true,
            frozen: false,
            approvals: BTreeMap::new(),
            next_approval_id: 0,
            meter: Meter::default(),
            sales: Vec::new(),
            closed_sales: 0,
            closed_max_epoch: None,
            closed_max_cum: 0,
            next_sale_id: 1,
            claimable_sum: 0,
            claimable_accounts: 0,
            owner_free: 0,
            held: 0,
            fee_to: Account::new(),
            paid_to_ever: 0,
            paid_to_this_epoch: 0,
            paid_to_dead_upto: 0,
            paid_to_dead_count: 0,
            open_auth_until: 0,
            base: base::BaseWallet::default(),
            limits: Limits::default(),
            treasury: 0,
            spent: Spent::default(),
            terms_min: TermsMin::PROD,
            keys_since: 0,
            key_batch: 0,
            recovered_buy: None,
            offers: Vec::new(),
            next_offer_id: 1,
            share_escrow: 0,
            config_commit: None,
            store: S::default(),
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

    /// No key batch is waiting for its callback.
    fn keys_settled(&self) -> Res<()> {
        if self.incoming.is_empty() && self.retiring.is_empty() {
            Ok(())
        } else {
            Err(Error::KeysPending)
        }
    }

    /// Escrow still owed across all sales.
    pub fn escrow_left(&self) -> Amount {
        self.sales.iter().map(Sale::remaining).sum()
    }

    /// SC14: upfront parts of sealed sales held until the delivery clears
    /// (review 4 M1: counted in solvency and the exact ledger).
    pub fn upfront_held_total(&self) -> Amount {
        self.sales
            .iter()
            .filter_map(|s| s.config.as_ref())
            .map(|c| c.upfront_held)
            .sum()
    }

    pub fn claimable_total(&self) -> Amount {
        self.claimable_sum
    }

    /// `payer` has no row of its own.
    pub fn store_payer_missing(&self, payer: &str) -> bool {
        self.store.payer(payer).is_none()
    }

    /// Give `payer` its own payer row (A49): the contract takes a NEAR
    /// storage deposit for it first. `false` if already registered.
    pub fn register_payer(&mut self, payer: &str) -> bool {
        self.meter.register(&mut self.store, payer)
    }

    /// Remove `payer`'s row if it never had a payment (A50).
    pub fn unregister_payer(&mut self, payer: &str) -> Result<(), &'static str> {
        self.meter.unregister(&mut self.store, payer)
    }

    /// `who`'s claimable balance.
    pub fn claimable_of(&self, who: &str) -> Amount {
        self.store.claimable(who)
    }

    fn credit(&mut self, who: &str, amount: Amount) {
        if amount > 0 {
            let had = self.store.claimable(who);
            if had == 0 {
                self.claimable_accounts += 1;
            }
            self.store.put_claimable(who, had + amount);
            self.claimable_sum += amount;
        }
    }

    /// A sale, open or finalized.
    pub fn sale(&self, id: u64) -> Option<Sale> {
        self.sales
            .iter()
            .find(|s| s.id == id)
            .cloned()
            .or_else(|| self.store.sale(id))
    }

    /// Move finalized sales out of the state blob into the history (A47).
    fn close_finalized_sales(&mut self) {
        let (done, open): (Vec<Sale>, Vec<Sale>) = std::mem::take(&mut self.sales)
            .into_iter()
            .partition(|s| s.finalized);
        self.sales = open;
        for s in done {
            self.closed_sales += 1;
            if self.closed_max_epoch.is_none_or(|m| s.seller_epoch > m) {
                self.closed_max_epoch = Some(s.seller_epoch);
                self.closed_max_cum = s.paid_to_cum;
            }
            self.store.put_sale(&s);
        }
    }

    fn add_past_owner(&mut self, who: &str) {
        if !self.store.past_owner(who) {
            self.store.put_past_owner(who);
            self.past_owners += 1;
        }
    }

    pub fn is_past_owner(&self, who: &str) -> bool {
        self.store.past_owner(who)
    }

    /// The owner epoch `to` was first paid in, unless pruned (A45).
    pub fn paid_to_epoch(&self, to: &str) -> Option<u64> {
        self.store
            .paid_to(to)
            .filter(|e| *e > self.paid_to_dead_upto)
    }

    /// Payout destinations tracked (all owners, pruned ones excluded).
    pub fn paid_to_total(&self) -> u64 {
        self.paid_to_ever - self.paid_to_dead_count
    }

    // ----- keys -------------------------------------------------------------

    /// Owner replaces the runner key. Returns the keys to delete; the contract
    /// batches the `delete_key`s and `add_key(new)` on itself, then calls
    /// `runner_key_done` with the batch outcome. Until then the old keys are
    /// `retiring`, the new one `incoming`, and the runner cannot act (A25).
    pub fn set_runner_key(&mut self, caller: &str, new_key: &str, now: Ts) -> Res<Vec<Key>> {
        self.only_owner(caller)?;
        self.not_settling()?;
        if self.frozen {
            return Err(Error::Frozen);
        }
        if self.status == Status::Listed {
            // Changing keys while listed would invalidate what buyers scanned.
            return Err(Error::AlreadyListed);
        }
        // A key whose deletion is not confirmed may already be gone; putting
        // it in this all-or-nothing batch could fail every later rotation.
        // Wait for its callback, or `recover_keys` (A33).
        self.keys_settled()?;
        if self.known_keys.contains(new_key) {
            return Err(Error::KeyAlreadyKnown);
        }
        let old: Vec<Key> = self.known_keys.iter().cloned().collect();
        self.retiring.append(&mut self.known_keys);
        self.incoming.insert(new_key.to_string());
        self.paused = true;
        self.keys_since = now;
        self.key_batch += 1;
        Ok(old)
    }

    /// Callback of `set_runner_key`. `old` = the runner keys before,
    /// `deleted` = every key the batch deleted. On success the new key is the
    /// runner key; on failure (the batch is all-or-nothing) the old keys are
    /// restored. Either way the runner can act again: an honest rotation
    /// never freezes or bricks the agent.
    ///
    /// `batch` is the nonce `set_runner_key` gave the batch (`key_batch`).
    /// A callback for an older batch is stale (a `recover_keys` ran in
    /// between) and changes nothing; it returns `false` (A44).
    pub fn runner_key_done(
        &mut self,
        batch: u64,
        old: &[Key],
        deleted: &[Key],
        new_key: &str,
        ok: bool,
    ) -> bool {
        if batch != self.key_batch || !self.incoming.contains(new_key) {
            return false;
        }
        self.incoming.remove(new_key);
        if ok {
            for k in deleted {
                self.retiring.remove(k);
            }
            self.known_keys.insert(new_key.to_string());
        } else {
            for k in old {
                self.retiring.remove(k);
                self.known_keys.insert(k.clone());
            }
        }
        self.paused = self.known_keys.is_empty();
        true
    }

    /// Is `key` in any key set? A stale batch may have added a key that no
    /// set names; the contract deletes such a key (A44).
    pub fn key_tracked(&self, key: &str) -> bool {
        self.known_keys.contains(key) || self.incoming.contains(key) || self.retiring.contains(key)
    }

    /// Callback after a deletion of `keys`, each in its own receipt (deed
    /// transfer, recovery). Every receipt has run by then, and a `delete_key`
    /// fails only when the key is already gone, so none of them is on the
    /// account any more, whatever the results (A33).
    pub fn keys_deleted(&mut self, keys: &[Key]) {
        for k in keys {
            self.retiring.remove(k);
            self.incoming.remove(k);
        }
    }

    /// The owner clears key sets whose callback never ran (it failed after
    /// its key batch, A33). Allowed `RECOVER_AFTER_NS` after the batch
    /// started. Every `incoming` and `retiring` key is deleted, each in its
    /// own receipt; until `keys_deleted` they stay pending (`NotRunner`).
    /// The runner keys in `known_keys` stay; with none the agent is paused
    /// until the owner sets a key.
    pub fn recover_keys(&mut self, caller: &str, now: Ts) -> Res<Recovery> {
        self.only_owner(caller)?;
        self.not_settling()?;
        if self.incoming.is_empty() && self.retiring.is_empty() {
            return Err(Error::NothingToRecover);
        }
        if now < self.keys_since.saturating_add(RECOVER_AFTER_NS) {
            return Err(Error::RecoverTooEarly);
        }
        self.retiring.append(&mut self.incoming);
        self.keys_since = now;
        self.key_batch += 1;
        self.paused = self.known_keys.is_empty();
        Ok(Recovery {
            delete_keys: self.retiring.iter().cloned().collect(),
            sale: None,
        })
    }

    /// Anyone undoes a buy whose `on_handover` never ran (it failed after the
    /// key batch, A33), `RECOVER_AFTER_NS` after the buy started. A failed
    /// callback makes the token refund the buyer in full, so the sale did not
    /// happen: the seller keeps the deed, the listing is withdrawn, and every
    /// key the buy touched (the seller's runner keys and the buyer's key) is
    /// deleted. The seller sets a new runner key and lists again.
    pub fn recover_sale(&mut self, now: Ts) -> Res<Recovery> {
        let p = match (&self.status, &self.pending) {
            (Status::Settling, Some(p)) => p.clone(),
            _ => return Err(Error::NothingToRecover),
        };
        if now < p.started_at.saturating_add(RECOVER_AFTER_NS) {
            return Err(Error::RecoverTooEarly);
        }
        self.pending = None;
        self.held -= p.amount;
        self.incoming.remove(&p.buyer_pk);
        self.retiring.append(&mut self.known_keys);
        self.retiring.insert(p.buyer_pk.clone());
        self.listing = None;
        self.listing_version += 1;
        self.status = self.status_before_listing.clone();
        self.paused = true;
        self.keys_since = now;
        self.key_batch += 1;
        self.recovered_buy = Some(p.clone());
        Ok(Recovery {
            delete_keys: self.retiring.iter().cloned().collect(),
            sale: Some(p),
        })
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
        if self.sales.len() >= MAX_OPEN_SALES {
            return Err(Error::TooManyOpenSales);
        }
        if self.status == Status::Listed {
            return Err(Error::AlreadyListed);
        }
        self.keys_settled()?;
        if !self.approvals.is_empty() || self.open_auth_until > now {
            return Err(Error::OpenApproval);
        }
        if price < USD {
            return Err(Error::BadPrice);
        }
        terms.check(&self.terms_min)?;
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
            treasury_snapshot: self.treasury,
            config: self.config_commit.clone(),
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
        self.begin_buy_sealed(
            buyer,
            amount,
            listing_version,
            known_keys_hash,
            buyer_pk,
            None,
            gas_ok,
            now,
        )
    }

    /// `begin_buy` with the buyer's one-time X25519 delivery key (hex), which
    /// a listing with a sealed config requires (SC3).
    #[allow(clippy::too_many_arguments)]
    pub fn begin_buy_sealed(
        &mut self,
        buyer: &str,
        amount: Amount,
        listing_version: u64,
        known_keys_hash: &str,
        buyer_pk: &str,
        delivery_pk: Option<&str>,
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
        if self.treasury < listing.treasury_snapshot {
            return Err(Error::TreasuryChanged);
        }
        if self.open_auth_until > now || !self.approvals.is_empty() {
            return Err(Error::OpenApproval);
        }
        if !gas_ok {
            return Err(Error::NotEnoughGas);
        }
        let delivery_pk = match (&listing.config, delivery_pk) {
            (Some(_), None) => return Err(Error::DeliveryKeyRequired),
            (Some(_), Some(pk)) => match sealed::hex32(pk) {
                Some(b) if sealed::usable_key(&b) => Some(hex::encode(b)),
                _ => return Err(Error::BadDeliveryKey),
            },
            (None, _) => None,
        };
        self.keys_settled()?;
        if self.known_keys.contains(buyer_pk) {
            return Err(Error::KeyAlreadyKnown);
        }
        let sale_id = self.next_sale_id;
        self.next_sale_id += 1;
        let prev_keys: Vec<Key> = self.known_keys.iter().cloned().collect();
        self.held += amount;
        self.status = Status::Settling;
        // The batch adds this key before `finish_buy` runs (A25).
        self.incoming.insert(buyer_pk.to_string());
        self.pending = Some(Pending {
            sale_id,
            buyer: buyer.to_string(),
            amount,
            buyer_pk: buyer_pk.to_string(),
            listing,
            prev_keys: prev_keys.clone(),
            started_at: now,
            delivery_pk,
        });
        Ok(BuyPlan {
            sale_id,
            delete_keys: prev_keys,
            add_key: buyer_pk.to_string(),
        })
    }

    /// Callback after the self key batch. `keys_ok` is the batch outcome.
    /// `on_handover` for the buy `recover_sale` undid (A44). It returns the
    /// whole payment so the token refunds the buyer (`recover_sale` already
    /// took it out of `held`). If its batch did run, it added the buyer key
    /// after the recovery deleted it: that key goes to `retiring` and is
    /// returned for deletion, so a signature by it is `NotRunner`, never a
    /// backdoor. `None` if `sale_id` is not the recovered buy.
    pub fn stale_handover(&mut self, sale_id: u64, keys_ok: bool) -> Option<(Amount, Option<Key>)> {
        if self.pending.as_ref().is_some_and(|p| p.sale_id == sale_id) {
            return None;
        }
        let p = self.recovered_buy.take_if(|p| p.sale_id == sale_id)?;
        Some((
            p.amount,
            keys_ok
                .then(|| self.retire_untracked(&p.buyer_pk))
                .flatten(),
        ))
    }

    /// A stale batch may have added `key` after a recovery deleted it. If no
    /// key set names it, mark it `retiring` and return it for deletion (A44).
    pub fn retire_untracked(&mut self, key: &str) -> Option<Key> {
        if self.key_tracked(key) {
            return None;
        }
        self.retiring.insert(key.to_string());
        Some(key.to_string())
    }

    pub fn finish_buy(&mut self, sale_id: u64, keys_ok: bool, now: Ts) -> Res<BuyResult> {
        let p = match &self.pending {
            Some(p) if p.sale_id == sale_id && self.status == Status::Settling => p.clone(),
            _ => return Err(Error::WrongSettlement),
        };
        self.pending = None;
        self.incoming.remove(&p.buyer_pk);
        if self.frozen {
            // A34: a hidden key was proven while settling. Nobody should
            // receive a permanently frozen agent: undo the buy, refund in full.
            self.held -= p.amount;
            self.listing = None;
            self.listing_version += 1;
            self.status = self.status_before_listing.clone();
            let mut delete_keys = Vec::new();
            if keys_ok {
                // The batch deleted the seller's keys and added the buyer's.
                self.known_keys.clear();
                self.retiring.insert(p.buyer_pk.clone());
                delete_keys.push(p.buyer_pk.clone());
                self.keys_since = now;
            }
            return Ok(BuyResult::RolledBack {
                unused: p.amount,
                frozen: true,
                delete_keys,
            });
        }
        if !keys_ok {
            // Roll back: listing, keys and owner were never changed.
            self.status = Status::Listed;
            self.held -= p.amount;
            return Ok(BuyResult::RolledBack {
                unused: p.amount,
                frozen: false,
                delete_keys: vec![],
            });
        }
        let l = &p.listing;
        let (paid_now, escrow) = money::split_price(l.price, l.terms.earnout_bps);
        let unused = p.amount - l.price;
        // A29: the seller's unwithdrawn revenue stays the seller's.
        let seller_revenue = std::mem::take(&mut self.owner_free);
        self.credit(&l.seller, seller_revenue);
        // Deed moves, owner epoch bumps, seller keys are gone.
        let seller = self.owner.clone();
        self.add_past_owner(&seller);
        self.owner = p.buyer.clone();
        self.trim_unsold_offers();
        self.owner_epoch += 1;
        self.paid_to_this_epoch = 0;
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
            seller_epoch: self.owner_epoch - 1,
            started_at: now,
            window_end: now + l.terms.window_secs * NANOS_PER_SEC,
            finalized: false,
            cut: false,
            paid_to_cum: self.paid_to_ever,
            config: l.config.clone().map(|commit| sealed::SaleConfig {
                commit,
                delivery_pk: p.delivery_pk.clone().unwrap_or_default(),
                deliver_by: now + sealed::DELIVER_WINDOW_NS,
                state: sealed::Delivery::Awaited,
                upfront_held: paid_now,
                cleared: false,
                upfront_returned: 0,
            }),
        };
        self.sales.push(sale.clone());
        // The bundle was sold with this sale; a new owner commits their own.
        self.config_commit = None;
        // The protocol fee stays held, as the fee account's claimable balance.
        // Out of the upfront part only, so never more than it (PR2): a
        // pay-from-revenue sale has no upfront part and takes no fee.
        // SC14: a sealed sale pays nothing now; the upfront part (and its
        // fee) waits for the delivery to be accepted.
        let (fee, payout) = if l.config.is_some() {
            (0, 0)
        } else {
            let fee = self.take_fee(l.price, paid_now);
            (fee, paid_now - fee)
        };
        // The overpay goes back through ft_resolve_transfer; the payout leaves now.
        self.held -= unused + payout;
        Ok(BuyResult::Done {
            sale: Box::new(sale),
            seller: l.seller.clone(),
            payout,
            fee,
            unused,
            seller_revenue,
        })
    }

    // ----- token transfers out ----------------------------------------------

    /// A transfer started by the contract finished. On failure the amount is
    /// credited back (push falls back to pull, E6). `by` is who started an
    /// owner withdrawal: if the deed moved before the callback, the money is
    /// still theirs, as a claimable balance, not the new owner's (A38).
    pub fn transfer_done(
        &mut self,
        kind: TransferKind,
        to: &str,
        by: Option<&str>,
        amount: Amount,
        ok: bool,
    ) {
        if ok {
            return;
        }
        self.held += amount;
        match kind {
            TransferKind::SellerPayout | TransferKind::Claim => self.credit(to, amount),
            TransferKind::OwnerWithdraw => match by {
                Some(b) if b != self.owner => self.credit(b, amount),
                _ => self.owner_free += amount,
            },
            // A failed supplier payment goes back to the treasury. The day's
            // spending is not reduced (the cap counts attempts, conservative).
            TransferKind::TreasuryPay => self.treasury += amount,
        }
    }

    /// Anyone with a claimable balance pulls it.
    pub fn claim(&mut self, caller: &str) -> Res<Amount> {
        let amount = self.store.claimable(caller);
        if amount == 0 {
            return Err(Error::NothingToClaim);
        }
        self.store.put_claimable(caller, 0);
        self.claimable_sum -= amount;
        self.claimable_accounts -= 1;
        self.held -= amount;
        Ok(amount)
    }

    /// Owner takes revenue out, to themselves or another account.
    /// On a frozen agent the runner can never spend the treasury again, so
    /// the owner may withdraw it too (A34): revenue first, then treasury.
    pub fn withdraw(&mut self, caller: &str, amount: Amount, to: &str) -> Res<Amount> {
        self.only_owner(caller)?;
        self.not_settling()?;
        // STR-12: while Listed the buyer has scanned the books and the NEAR
        // storage; a payout to a fresh address adds a row the scan did not
        // reserve. Owner withdrawals wait until the listing ends.
        if self.status == Status::Listed {
            return Err(Error::SpendingPaused);
        }
        let treasury = if self.frozen { self.treasury } else { 0 };
        if amount == 0 || amount > self.owner_free + treasury {
            return Err(Error::InsufficientFunds);
        }
        self.note_paid_to(to)?;
        let from_free = amount.min(self.owner_free);
        self.owner_free -= from_free;
        self.treasury -= amount - from_free;
        self.held -= amount;
        Ok(amount)
    }

    /// Remember who the agent paid, for the `round_trip` flag (A30). The set
    /// is bounded but never silently stops tracking: once full, sending to a
    /// new account is refused (`PaidToFull`). The owner, past owners and the
    /// agent need no entry: their payments are flagged `self` anyway, so the
    /// owner can always withdraw to themselves (I10).
    fn note_paid_to(&mut self, to: &str) -> Res<()> {
        if self.flags_for(to).self_pay || self.paid_to_epoch(to).is_some() {
            return Ok(());
        }
        if self.paid_to_this_owner() >= MAX_PAID_TO
            || self.paid_to_total() as usize >= MAX_PAID_TO_TOTAL
        {
            return Err(Error::PaidToFull);
        }
        // A stored entry of a pruned epoch is overwritten.
        self.store.put_paid_to(to, self.owner_epoch);
        self.paid_to_ever += 1;
        self.paid_to_this_epoch += 1;
        Ok(())
    }

    /// Payout destinations the current owner added (A36).
    pub fn paid_to_this_owner(&self) -> usize {
        self.paid_to_this_epoch as usize
    }

    /// Does a payment by `payer` release from `sale`'s earn-out? Not if it is
    /// a self or sub-account payment, and not if the agent paid `payer` while
    /// the seller (or an owner before them) held the deed. A round trip the
    /// buyer creates after the sale does not count against the seller (A31).
    pub fn releases_for(&self, payer: &str, flags: &Flags, sale: &Sale) -> bool {
        // SC4/SC14: nothing is released before the sealed config is
        // delivered and accepted (or the dispute window closed undisputed).
        sale.config.as_ref().is_none_or(|c| c.cleared)
            && !flags.self_pay
            && !flags.seller_subaccount
            && !self
                .paid_to_epoch(payer)
                .is_some_and(|epoch| epoch <= sale.seller_epoch)
    }

    // ----- payments and earn-out --------------------------------------------

    /// Classify a payment by who paid.
    pub fn flags_for(&self, payer: &str) -> Flags {
        // One lookup per name and per parent of the payer, never a scan of
        // all past owners (A47).
        let ours = |a: &str| a == self.agent || a == self.owner || self.store.past_owner(a);
        // C30: an ancestor of the agent's account (`alice.testnet` for
        // `bot.alice.testnet`) created it and is the seller's own money.
        let ancestor = self.agent.len() > payer.len() + 1
            && self.agent.ends_with(payer)
            && self.agent.as_bytes()[self.agent.len() - payer.len() - 1] == b'.';
        let is_self = ours(payer) || ancestor;
        let sub = payer
            .char_indices()
            .filter(|(_, c)| *c == '.')
            .any(|(i, _)| i + 1 < payer.len() && ours(&payer[i + 1..]));
        Flags {
            self_pay: is_self,
            seller_subaccount: !is_self && sub,
            round_trip: !is_self && !sub && self.paid_to_epoch(payer).is_some(),
        }
    }

    /// Record an incoming payment. `verified` is the contract's NEAR AI
    /// signature check; it only counts for a counted payment (I9).
    pub fn pay(&mut self, payer: &str, amount: Amount, verified: bool, now: Ts) -> PayResult {
        // C32: a frozen agent proved a backdoor; it takes no new revenue.
        if self.frozen {
            return PayResult::Refund("Frozen");
        }
        if amount < meter::MIN_PAYMENT {
            return PayResult::Refund("Dust");
        }
        if amount < self.limits.price_floor {
            return PayResult::Refund("PolicyViolation:PriceFloor");
        }
        let flags = self.flags_for(payer);
        let counted = !flags.any();
        let verified = counted && verified;
        self.meter
            .record(&mut self.store, payer, amount, flags, verified, now);
        self.held += amount;
        // Investors first, from the gross payment, whoever paid (T3, SH8):
        // a cheater's own payment pays them too.
        let (to_investors, share_released) = self.pay_investors(amount, counted, now);
        let (to_owner, to_treasury) =
            money::split_revenue(amount - to_investors, self.limits.treasury_bps);
        self.owner_free += to_owner;
        self.treasury += to_treasury;
        let mut released = Vec::new();
        // Release from the oldest open earn-out, at most share_bps of this
        // payment in total (S10: wash-paying always loses money).
        if let Some(idx) = self
            .sales
            .iter()
            .position(|s| !s.finalized && s.remaining() > 0 && now < s.window_end)
        {
            if self.releases_for(payer, &flags, &self.sales[idx]) {
                let sale = &mut self.sales[idx];
                let r = money::release_for_payment(amount, sale.share_bps, sale.remaining());
                if r > 0 {
                    sale.released += r;
                    released.push((sale.id, sale.seller.clone(), r));
                }
            }
        }
        for (_, seller, r) in &released {
            self.credit(seller, *r);
        }
        PayResult::Recorded {
            flags,
            counted,
            verified,
            released,
            to_owner,
            to_treasury,
            to_investors,
            share_released,
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
        // T4: while a slice is open (sold or not), the owner cannot turn
        // customers away by raising the price floor; checking only holders
        // let the owner front-run the first buy (review M1).
        if limits.price_floor > self.limits.price_floor && !self.offers.is_empty() {
            return Err(Error::SharesOpen);
        }
        self.limits = limits;
        Ok(())
    }

    pub fn spent_today(&self, now: Ts) -> Amount {
        self.spent.today(self.owner_epoch, now / DAY_NS)
    }

    /// The runner pays `payee` out of the treasury. Call only after
    /// `runner_guard` returned `Ok`. Checks, in order: payee allowed, daily
    /// cap, treasury balance (and never while listed). Nothing changes on error.
    pub fn spend(&mut self, payee: &str, amount: Amount, now: Ts) -> Res<Amount> {
        // A24: no spending while listed, so the seller cannot drain the
        // treasury the buyer is paying for.
        if self.status == Status::Listed {
            return Err(Error::SpendingPaused);
        }
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
        self.note_paid_to(payee)?;
        self.treasury -= amount;
        self.held -= amount;
        self.spent = Spent {
            owner_epoch: self.owner_epoch,
            day,
            amount: spent + amount,
        };
        Ok(spent + amount)
    }

    // ----- sealed agent config (sealed.rs) ------------------------------------

    /// Owner commits to the sealed config the next listing sells (`None`
    /// clears it). Not while listed: the buyer scanned the listing (SC1).
    pub fn commit_config(&mut self, caller: &str, commit: Option<sealed::ConfigCommit>) -> Res<()> {
        self.only_owner(caller)?;
        self.not_settling()?;
        if self.status == Status::Listed {
            return Err(Error::AlreadyListed);
        }
        if commit.as_ref().is_some_and(|c| !c.valid()) {
            return Err(Error::BadConfig);
        }
        self.config_commit = commit;
        Ok(())
    }

    fn sale_config(&self, sale_id: u64) -> Res<(usize, &sealed::SaleConfig)> {
        let idx = self
            .sales
            .iter()
            .position(|s| s.id == sale_id && !s.finalized)
            .ok_or(Error::UnknownSale)?;
        let c = self.sales[idx]
            .config
            .as_ref()
            .ok_or(Error::NoSealedConfig)?;
        Ok((idx, c))
    }

    /// The seller posts the bundle sealed to the buyer's delivery key. The
    /// contract keeps the ciphertext; here only its hash (SC3). The
    /// ciphertext is as long as the committed bundle.
    pub fn deliver_config(
        &mut self,
        caller: &str,
        sale_id: u64,
        ephemeral_pk: &str,
        cipher: &[u8],
        now: Ts,
    ) -> Res<()> {
        let (idx, c) = self.sale_config(sale_id)?;
        if caller != self.sales[idx].seller {
            return Err(Error::NotSeller);
        }
        if !matches!(c.state, sealed::Delivery::Awaited) {
            return Err(Error::AlreadyDelivered);
        }
        if now > c.deliver_by {
            return Err(Error::DeliveryLate);
        }
        if cipher.len() as u32 != c.commit.bytes {
            return Err(Error::BadConfig);
        }
        // Review 4 H2: a low-order ephemeral key opens to nothing in any
        // client (Node's X25519 throws), so the buyer could not dispute it.
        let eph = sealed::hex32(ephemeral_pk)
            .filter(sealed::usable_key)
            .ok_or(Error::BadConfig)?;
        let cfg = self.sales[idx].config.as_mut().expect("checked");
        cfg.state = sealed::Delivery::Delivered {
            ephemeral_pk: hex::encode(eph),
            cipher_hash: hex::encode(Sha256::digest(cipher)),
            at: now,
        };
        Ok(())
    }

    /// No delivery by the deadline: the buyer takes the whole remaining
    /// escrow back (SC5). Anyone may call; the money goes to the buyer.
    pub fn claim_undelivered(&mut self, sale_id: u64, now: Ts) -> Res<Amount> {
        let (idx, c) = self.sale_config(sale_id)?;
        if !matches!(c.state, sealed::Delivery::Awaited) {
            return Err(Error::AlreadyDelivered);
        }
        if now <= c.deliver_by {
            return Err(Error::DeliveryOpen);
        }
        self.config_refund(idx, false).map(|(_, r)| r)
    }

    /// The buyer reveals the one-time secret key; the contract decrypts the
    /// stored ciphertext and compares it with the commitment (SC8-SC10).
    /// Not the committed bundle: the remaining escrow goes back to the buyer.
    pub fn dispute_config(
        &mut self,
        caller: &str,
        sale_id: u64,
        delivery_sk: &str,
        cipher: &[u8],
        now: Ts,
    ) -> Res<Amount> {
        let (idx, c) = self.sale_config(sale_id)?;
        if caller != self.sales[idx].buyer {
            return Err(Error::NotBuyer);
        }
        let sealed::Delivery::Delivered {
            ephemeral_pk,
            cipher_hash,
            ..
        } = &c.state
        else {
            return Err(Error::DisputeClosed);
        };
        if c.cleared || now > c.dispute_until() {
            return Err(Error::DisputeClosed);
        }
        let sk = sealed::hex32(delivery_sk).ok_or(Error::BadDeliveryKey)?;
        if hex::encode(sealed::public_key(&sk)) != c.delivery_pk {
            return Err(Error::BadDeliveryKey);
        }
        if hex::encode(Sha256::digest(cipher)) != *cipher_hash {
            // The contract layer passes the stored ciphertext; anything else
            // is a wiring bug, not the seller's fault.
            return Err(Error::BadConfig);
        }
        let eph = sealed::hex32(ephemeral_pk).ok_or(Error::BadConfig)?;
        if sealed::delivery_matches(&sk, &eph, cipher, &c.commit) {
            return Err(Error::ConfigMatches);
        }
        self.config_refund(idx, true).map(|(_, r)| r)
    }

    /// SC14: the buyer accepts the delivered bundle, or anyone calls after
    /// the dispute window closed undisputed. The held upfront part (less the
    /// 1% fee) goes to the seller and earn-out releases start. Returns the
    /// seller's payout.
    pub fn release_upfront(&mut self, caller: &str, sale_id: u64, now: Ts) -> Res<Amount> {
        let (idx, c) = self.sale_config(sale_id)?;
        if c.cleared {
            return Ok(0);
        }
        if !c.delivered() {
            return Err(Error::DeliveryOpen);
        }
        if caller != self.sales[idx].buyer && now <= c.dispute_until() {
            return Err(Error::DisputeOpen);
        }
        Ok(self.clear_upfront(idx))
    }

    fn clear_upfront(&mut self, idx: usize) -> Amount {
        let price = self.sales[idx].price;
        let seller = self.sales[idx].seller.clone();
        let c = self.sales[idx].config.as_mut().expect("sealed sale");
        let upfront = std::mem::take(&mut c.upfront_held);
        c.cleared = true;
        let fee = self.take_fee(price, upfront);
        self.pay_seller(&seller, upfront - fee);
        upfront - fee
    }

    /// Close sale `idx` with the remaining escrow, and the held upfront
    /// part (SC14), to the buyer.
    fn config_refund(&mut self, idx: usize, mismatch: bool) -> Res<(Sale, Amount)> {
        let s = &mut self.sales[idx];
        let rest = s.remaining();
        s.returned += rest;
        let upfront = s.config.as_mut().map_or(0, |c| {
            let u = std::mem::take(&mut c.upfront_held);
            c.upfront_returned += u;
            u
        });
        let r = rest + upfront;
        s.finalized = true;
        s.cut = true;
        if let Some(c) = s.config.as_mut() {
            c.state = if mismatch {
                sealed::Delivery::Mismatch { refunded: r }
            } else {
                sealed::Delivery::Missed { refunded: r }
            };
        }
        let s = s.clone();
        self.credit(&s.buyer, r);
        self.close_finalized_sales();
        self.prune_paid_to();
        Ok((s, r))
    }

    /// Close an earn-out after its window. Anyone may call; a second call is a
    /// no-op that returns the same `Sale` (E21).
    pub fn finalize_earnout(&mut self, sale_id: u64, now: Ts) -> Res<Sale> {
        let Some(idx) = self.sales.iter().position(|s| s.id == sale_id) else {
            // Finalized before: the same answer again (E21).
            return self.store.sale(sale_id).ok_or(Error::UnknownSale);
        };
        if now < self.sales[idx].window_end {
            return Err(Error::WindowOpen);
        }
        if let Some(c) = &self.sales[idx].config {
            // SC7: a delivery can still be disputed; SC6: an undelivered
            // bundle earns the seller no floor.
            if now < c.dispute_until() {
                return Err(Error::WindowOpen);
            }
            if !c.delivered() {
                // Judge 19: the seller keeps the full delivery window even
                // when the earn-out window is shorter.
                if now <= c.deliver_by {
                    return Err(Error::DeliveryOpen);
                }
                return self.config_refund(idx, false).map(|(s, _)| s);
            }
            if !c.cleared {
                self.clear_upfront(idx);
            }
        }
        let s = &mut self.sales[idx];
        let f = money::finalize(s.escrow, s.released + s.returned, s.floor_bps);
        s.released += f.seller_topup;
        s.returned += f.buyer_refund;
        s.finalized = true;
        let s = s.clone();
        self.credit(&s.seller, f.seller_topup);
        self.credit(&s.buyer, f.buyer_refund);
        self.close_finalized_sales();
        self.prune_paid_to();
        Ok(s)
    }

    /// Drop payout destinations no earn-out can still ask about (A45). An
    /// entry of epoch `e` matters only to an open sale whose seller epoch is
    /// `>= e` (`releases_for`). Once no sale is open, every entry up to the
    /// newest finalized sale's era is dropped. Entries of owners not yet
    /// separated by a real sale stay, so moving the deed between one's own
    /// accounts never clears the table. Pruning always drops a prefix of
    /// epochs, so it is a watermark plus a count, O(1) in the blob (A53).
    /// While any sale is open nothing is dropped (every older epoch may be
    /// asked about), which keeps more entries than strictly needed when
    /// sales finalize out of order: that only flags more payments as round
    /// trips, never fewer.
    pub fn prune_paid_to(&mut self) {
        let Some(sold) = self.closed_max_epoch else {
            return;
        };
        if self.sales.iter().any(|s| !s.finalized) || sold <= self.paid_to_dead_upto {
            return;
        }
        self.paid_to_dead_upto = sold;
        self.paid_to_dead_count = self.closed_max_cum;
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
        now: Ts,
    ) -> Res<Vec<Key>> {
        self.not_settling()?;
        if !self.incoming.is_empty() {
            return Err(Error::KeysPending);
        }
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
        // Review 2: a deed held by the agent's own account could never move
        // again (the contract has no key to sign for itself).
        if receiver == self.agent {
            return Err(Error::DeedToAgent);
        }
        // C28: a Base authorization signed (or being signed) by the agent's
        // wallet must not travel with the deed to a new owner.
        if self.open_auth_until > now {
            return Err(Error::OpenApproval);
        }
        self.approvals.clear();
        if self.status == Status::Listed {
            self.listing = None;
            self.status = self.status_before_listing.clone();
        }
        self.listing_version += 1;
        let old = self.owner.clone();
        // A48: as in a sale (A29), the old owner's unwithdrawn revenue stays
        // theirs; whoever moved the deed cannot withdraw it.
        let revenue = std::mem::take(&mut self.owner_free);
        self.credit(&old, revenue);
        self.add_past_owner(&old);
        self.owner = receiver.to_string();
        self.trim_unsold_offers();
        // Review 4 L2: as in a sale, a sealed-config commitment does not pass
        // with the deed; the new owner commits their own bundle.
        self.config_commit = None;
        self.owner_epoch += 1;
        self.paid_to_this_epoch = 0;
        // The keys stay `retiring` until `keys_deleted` confirms (A25).
        let keys: Vec<Key> = self.known_keys.iter().cloned().collect();
        self.retiring.append(&mut self.known_keys);
        self.paused = true;
        self.keys_since = now;
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
        // A25: a key being added or deleted by a batch whose callback has not
        // run is not a runner key, but not a backdoor either.
        if self.incoming.contains(signer_pk) || self.retiring.contains(signer_pk) {
            return Err(Error::NotRunner);
        }
        if !self.known_keys.contains(signer_pk) {
            self.frozen = true;
            let mut returned = Vec::new();
            for s in self.sales.iter_mut().filter(|s| !s.finalized) {
                // SC14 (review 4 H1): a sealed sale not yet accepted still
                // holds its upfront part; it goes back to the buyer too.
                let upfront = s.config.as_mut().map_or(0, |c| {
                    let u = std::mem::take(&mut c.upfront_held);
                    c.upfront_returned += u;
                    u
                });
                let r = s.remaining() + upfront;
                s.returned += r - upfront;
                s.finalized = true;
                s.cut = true;
                returned.push((s.id, s.buyer.clone(), r));
            }
            for (_, buyer, r) in &returned {
                self.credit(buyer, *r);
            }
            self.close_finalized_sales();
            // A proven backdoor also ends every share slice: the investors
            // get their price escrow back.
            self.end_all_offers();
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
        self.held
            >= self.escrow_left()
                + self.upfront_held_total()
                + self.claimable_total()
                + self.owner_free
                + self.treasury
                + self.share_escrow
    }

    pub fn earnings(&self, now: Ts) -> Earnings {
        self.meter.earnings(&self.store, now)
    }

    // ----- shares of future earnings (PLAN-shares.md) -------------------------

    fn holder_count(&self) -> usize {
        self.offers.iter().map(|o| o.holders.len()).sum()
    }

    /// Share of every payment promised by the open slices (sold or not).
    pub fn shares_bps(&self) -> u16 {
        self.offers.iter().map(|o| o.bps).sum()
    }

    pub fn offer(&self, id: u64) -> Option<&ShareOffer> {
        self.offers.iter().find(|o| o.id == id)
    }

    /// Pay the escrow of a slice to its seller: their revenue if they still
    /// own the agent, their claimable balance if they sold it (T9).
    /// Credit the protocol fee on `base`, at most `cap` (the part it comes
    /// out of), to `fee_to`; 0 when no fee account. What is credited is
    /// exactly what is returned, so the ledger stays whole.
    fn take_fee(&mut self, base: Amount, cap: Amount) -> Amount {
        if self.fee_to.is_empty() {
            return 0;
        }
        let fee = money::protocol_fee(base).min(cap);
        if fee == 0 {
            return 0;
        }
        let to = self.fee_to.clone();
        self.credit(&to, fee);
        fee
    }

    fn pay_seller(&mut self, seller: &str, amount: Amount) {
        if amount == 0 {
            return;
        }
        if seller == self.owner {
            self.owner_free += amount;
        } else {
            self.credit(seller, amount);
        }
    }

    /// Credit each holder's part of a gross payment and release the matching
    /// price escrow to the slice's seller (SH2, SH11). Only credits, never a
    /// transfer, so `pay()` stays bounded (T15). A flagged payment still pays
    /// holders (SH8) but releases no escrow, so a self-payment cannot unlock
    /// the held half of the price (review M2).
    fn pay_investors(
        &mut self,
        amount: Amount,
        counted: bool,
        now: Ts,
    ) -> (Amount, Vec<(Account, Amount)>) {
        let mut total = 0;
        let mut credits: Vec<(Account, Amount)> = Vec::new();
        let mut releases: Vec<(Account, Amount)> = Vec::new();
        for o in self.offers.iter_mut().filter(|o| o.active(now)) {
            let mut released = 0;
            for h in o.holders.iter_mut() {
                let cut = shares::holder_cut(amount, h.bps);
                if cut == 0 {
                    continue;
                }
                h.earned += cut;
                total += cut;
                credits.push((h.account.clone(), cut));
                let r = if counted { cut.min(h.escrow) } else { 0 };
                h.escrow -= r;
                released += r;
            }
            if released > 0 {
                releases.push((o.seller.clone(), released));
            }
        }
        for (who, cut) in &credits {
            self.credit(who, *cut);
        }
        for (seller, r) in &releases {
            self.share_escrow -= r;
            self.pay_seller(seller, *r);
        }
        (total, releases)
    }

    /// Owner offers a slice: `bps` of every payment until `end`, for `price`
    /// (SH1, SH6).
    pub fn offer_share(
        &mut self,
        caller: &str,
        bps: u16,
        price: Amount,
        end: Ts,
        now: Ts,
    ) -> Res<u64> {
        self.only_owner(caller)?;
        self.not_settling()?;
        if self.frozen {
            return Err(Error::Frozen);
        }
        if self.status == Status::Listed {
            return Err(Error::AlreadyListed);
        }
        if bps < shares::MIN_OFFER_BPS
            || price == 0
            || end < now.saturating_add(shares::MIN_TERM_NS)
            || end > now.saturating_add(shares::MAX_TERM_NS)
        {
            return Err(Error::BadShareTerms);
        }
        // Ended slices stop counting once closed; until then they count.
        // Only this owner's own slices count, so slices inherited from a
        // seller cannot use up a buyer's slots (review 2 M1).
        if self
            .offers
            .iter()
            .filter(|o| o.seller == self.owner)
            .count()
            >= shares::MAX_OPEN_OFFERS
        {
            return Err(Error::TooManyOffers);
        }
        if self.shares_bps() as u32 + bps as u32 > shares::MAX_SHARES_BPS as u32 {
            return Err(Error::SharesTooHigh);
        }
        let id = self.next_offer_id;
        self.next_offer_id += 1;
        self.offers.push(ShareOffer {
            id,
            seller: self.owner.clone(),
            bps,
            price,
            created: now,
            end,
            sold_bps: 0,
            holders: Vec::new(),
        });
        Ok(id)
    }

    /// Owner withdraws a slice nobody bought yet. Once any part is sold the
    /// terms are fixed (T7).
    pub fn cancel_offer(&mut self, caller: &str, id: u64) -> Res<()> {
        self.only_owner(caller)?;
        let i = self
            .offers
            .iter()
            .position(|o| o.id == id)
            .ok_or(Error::UnknownOffer)?;
        if self.offers[i].sold_bps > 0 {
            return Err(Error::SharesOpen);
        }
        self.offers.remove(i);
        Ok(())
    }

    /// An investor's tokens arrive (`ft_on_transfer`, msg `buy_share`):
    /// buy `part` bps of the slice the investor saw. Returns the part's cost;
    /// the rest of `amount` goes back through `ft_resolve_transfer`.
    pub fn buy_share(
        &mut self,
        buyer: &str,
        amount: Amount,
        seen: ShareTerms,
        part: u16,
        now: Ts,
    ) -> Res<Amount> {
        if self.frozen {
            return Err(Error::Frozen);
        }
        // The terms a sale buyer scanned include the open slices (SH6).
        if matches!(self.status, Status::Listed | Status::Settling) {
            return Err(Error::SaleInProgress);
        }
        let f = self.flags_for(buyer);
        if f.self_pay || f.seller_subaccount {
            return Err(Error::SelfBuy);
        }
        let holders = self.holder_count();
        let o = self
            .offers
            .iter_mut()
            .find(|o| o.id == seen.offer_id)
            .ok_or(Error::UnknownOffer)?;
        if o.terms() != seen {
            return Err(Error::ShareChanged);
        }
        if !o.active(now) || o.sold_bps >= o.bps {
            return Err(Error::OfferClosed);
        }
        if part == 0 || part > o.bps - o.sold_bps {
            return Err(Error::BadShareTerms);
        }
        if part < o.min_part() {
            return Err(Error::ShareTooSmall);
        }
        let cost = o.cost(part);
        if amount < cost {
            return Err(Error::Underpaid);
        }
        let (now_part, escrow) = shares::split_cost(cost);
        match o.holders.iter_mut().find(|h| h.account == buyer) {
            Some(h) => {
                h.bps += part;
                h.paid_in += cost;
                h.escrow += escrow;
            }
            None => {
                if holders >= shares::MAX_HOLDERS {
                    return Err(Error::TooManyHolders);
                }
                o.holders.push(Holding {
                    account: buyer.to_string(),
                    bps: part,
                    paid_in: cost,
                    escrow,
                    earned: 0,
                });
            }
        }
        o.sold_bps += part;
        let seller = o.seller.clone();
        self.held += cost;
        self.share_escrow += escrow;
        let fee = self.take_fee(cost, now_part);
        self.pay_seller(&seller, now_part - fee);
        Ok(cost)
    }

    /// After a slice's end anyone may close it: each investor gets the price
    /// escrow still held back (SH4, SH11). Returns the refunds.
    pub fn close_share(&mut self, id: u64, now: Ts) -> Res<Vec<(Account, Amount)>> {
        let i = self
            .offers
            .iter()
            .position(|o| o.id == id)
            .ok_or(Error::UnknownOffer)?;
        if self.offers[i].active(now) {
            return Err(Error::WindowOpen);
        }
        let o = self.offers.remove(i);
        Ok(self.refund_offer(o))
    }

    fn refund_offer(&mut self, o: ShareOffer) -> Vec<(Account, Amount)> {
        let mut out = Vec::new();
        for h in o.holders {
            if h.escrow > 0 {
                self.share_escrow -= h.escrow;
                self.credit(&h.account, h.escrow);
                out.push((h.account, h.escrow));
            }
        }
        out
    }

    /// The deed moved: the unsold rest of every slice closes, so the old
    /// owner can no longer sell parts of the new owner's revenue (review H1).
    /// Sold parts stay; the buyer saw them in the scan (T9, SH7).
    fn trim_unsold_offers(&mut self) {
        self.offers.retain(|o| o.sold_bps > 0);
        for o in self.offers.iter_mut() {
            if o.bps != o.sold_bps {
                // Keep the price true to what was sold (review 2 L1).
                o.price = o.holders.iter().map(|h| h.paid_in).sum();
                o.bps = o.sold_bps;
            }
        }
    }

    fn end_all_offers(&mut self) {
        for o in std::mem::take(&mut self.offers) {
            self.refund_offer(o);
        }
    }
}
