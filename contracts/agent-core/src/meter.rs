//! Earnings meter: every payment counted by payer and UTC day, with flags.
//!
//! Storage per payment is bounded (I11): at most one new payer entry (until
//! `MAX_PAYERS`, then the `other` bucket) and at most one new day entry.

use crate::{Account, Amount, Ts, DAY_NS, USD};
use borsh::{BorshDeserialize, BorshSerialize};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// Smallest payment the meter records (0.10). Smaller ones are refunded (E15).
pub const MIN_PAYMENT: Amount = USD / 10;
/// Distinct payers tracked one by one; the rest are pooled as `other` (E16).
pub const MAX_PAYERS: usize = 1_000;
/// A counted payer whose first payment is newer than this is reported as
/// "fresh". Informational only: it does not change `counted` (see N3).
pub const FRESH_WINDOW_NS: u64 = 7 * DAY_NS;

#[derive(
    Debug,
    Clone,
    Copy,
    Default,
    PartialEq,
    Eq,
    Serialize,
    Deserialize,
    BorshSerialize,
    BorshDeserialize,
)]
#[cfg_attr(not(target_arch = "wasm32"), derive(borsh::BorshSchema))]
pub struct Flags {
    /// Paid by the agent itself, its owner or any past owner.
    #[serde(rename = "self")]
    pub self_pay: bool,
    /// Paid by a sub-account of the agent, the owner or a past owner.
    pub seller_subaccount: bool,
    /// Paid by an account the agent sent tokens to before.
    pub round_trip: bool,
}

impl Flags {
    pub fn any(&self) -> bool {
        self.self_pay || self.seller_subaccount || self.round_trip
    }
}

#[derive(
    Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, BorshSerialize, BorshDeserialize,
)]
#[cfg_attr(not(target_arch = "wasm32"), derive(borsh::BorshSchema))]
pub struct Bucket {
    pub gross: Amount,
    pub counted: Amount,
    pub verified: Amount,
    pub payments: u64,
}

impl Bucket {
    fn add(&mut self, amount: Amount, counted: bool, verified: bool) {
        self.gross += amount;
        if counted {
            self.counted += amount;
        }
        if verified {
            self.verified += amount;
        }
        self.payments += 1;
    }
}

#[derive(
    Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, BorshSerialize, BorshDeserialize,
)]
#[cfg_attr(not(target_arch = "wasm32"), derive(borsh::BorshSchema))]
pub struct PayerStats {
    pub totals: Bucket,
    pub first_ts: Ts,
    pub last_ts: Ts,
    pub flags: Flags,
}

/// Per-day totals, plus what the payers first seen that day did (for the
/// "fresh payers" figures without reading every payer).
#[derive(
    Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, BorshSerialize, BorshDeserialize,
)]
#[cfg_attr(not(target_arch = "wasm32"), derive(borsh::BorshSchema))]
pub struct DayStats {
    pub totals: Bucket,
    /// Tracked payers first seen this day that have a counted payment.
    pub new_counted_payers: u64,
    /// Everything those payers paid that was counted (on any day).
    pub counted_from_new: Amount,
}

/// Every table that outsiders or a long life can grow: payer rows, day
/// rows, payout destinations, claimable balances, past owners and the sale
/// history. The contract keeps each entry under its own storage key, read
/// one at a time, so loading the contract state never reads them and its
/// cost does not grow with them (A47). `MemStore` keeps them in memory for
/// the pure tests.
pub trait Store {
    fn payer(&self, account: &str) -> Option<PayerStats>;
    fn put_payer(&mut self, account: &str, stats: &PayerStats);
    fn day(&self, day: u64) -> Option<DayStats>;
    fn put_day(&mut self, day: u64, stats: &DayStats);
    fn remove_day(&mut self, day: u64);
    /// Day number at position `index` of the day list (absolute, oldest first).
    fn day_key(&self, index: u64) -> Option<u64>;
    fn put_day_key(&mut self, index: u64, day: u64);
    fn remove_day_key(&mut self, index: u64);
    /// Payout destination -> owner epoch of the first send (A30, A45).
    fn paid_to(&self, account: &str) -> Option<u64>;
    fn put_paid_to(&mut self, account: &str, epoch: u64);
    /// Claimable balance; 0 = no entry.
    fn claimable(&self, account: &str) -> Amount;
    fn put_claimable(&mut self, account: &str, amount: Amount);
    fn past_owner(&self, account: &str) -> bool;
    fn put_past_owner(&mut self, account: &str);
    /// Finalized sales (history), by id.
    fn sale(&self, id: u64) -> Option<crate::Sale>;
    fn put_sale(&mut self, sale: &crate::Sale);
}

/// In-memory `Store` (pure tests, property tests).
#[derive(
    Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, BorshSerialize, BorshDeserialize,
)]
#[cfg_attr(not(target_arch = "wasm32"), derive(borsh::BorshSchema))]
pub struct MemStore {
    pub payers: BTreeMap<Account, PayerStats>,
    pub days: BTreeMap<u64, DayStats>,
    pub day_keys: BTreeMap<u64, u64>,
    pub paid_to: BTreeMap<Account, u64>,
    pub claimable: BTreeMap<Account, Amount>,
    pub past_owners: std::collections::BTreeSet<Account>,
    pub sales: BTreeMap<u64, crate::Sale>,
}

impl Store for MemStore {
    fn payer(&self, account: &str) -> Option<PayerStats> {
        self.payers.get(account).cloned()
    }
    fn put_payer(&mut self, account: &str, stats: &PayerStats) {
        self.payers.insert(account.to_string(), stats.clone());
    }
    fn day(&self, day: u64) -> Option<DayStats> {
        self.days.get(&day).cloned()
    }
    fn put_day(&mut self, day: u64, stats: &DayStats) {
        self.days.insert(day, stats.clone());
    }
    fn remove_day(&mut self, day: u64) {
        self.days.remove(&day);
    }
    fn day_key(&self, index: u64) -> Option<u64> {
        self.day_keys.get(&index).copied()
    }
    fn put_day_key(&mut self, index: u64, day: u64) {
        self.day_keys.insert(index, day);
    }
    fn remove_day_key(&mut self, index: u64) {
        self.day_keys.remove(&index);
    }
    fn paid_to(&self, account: &str) -> Option<u64> {
        self.paid_to.get(account).copied()
    }
    fn put_paid_to(&mut self, account: &str, epoch: u64) {
        self.paid_to.insert(account.to_string(), epoch);
    }
    fn claimable(&self, account: &str) -> Amount {
        self.claimable.get(account).copied().unwrap_or(0)
    }
    fn put_claimable(&mut self, account: &str, amount: Amount) {
        if amount == 0 {
            self.claimable.remove(account);
        } else {
            self.claimable.insert(account.to_string(), amount);
        }
    }
    fn past_owner(&self, account: &str) -> bool {
        self.past_owners.contains(account)
    }
    fn put_past_owner(&mut self, account: &str) {
        self.past_owners.insert(account.to_string());
    }
    fn sale(&self, id: u64) -> Option<crate::Sale> {
        self.sales.get(&id).cloned()
    }
    fn put_sale(&mut self, sale: &crate::Sale) {
        self.sales.insert(sale.id, sale.clone());
    }
}

/// Day rows kept; older ones are dropped (their totals stay in `total`).
pub const MAX_DAYS: u64 = 400;
/// Day rows `get_earnings` returns (the newest); `get_earnings_days` pages
/// through the rest.
pub const EARNINGS_DAYS: u64 = 31;
/// A payer not tracked yet gets its own entry only with a payment of at
/// least this (1.00): the payment pays for the storage it adds. Smaller
/// first payments go to the `other` bucket (A47).
pub const MIN_NEW_PAYER: Amount = USD;

/// Aggregates only: everything here is O(1) in the number of payers and
/// days. Per-payer and per-day rows live in the `Store` (A47).
#[derive(
    Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, BorshSerialize, BorshDeserialize,
)]
#[cfg_attr(not(target_arch = "wasm32"), derive(borsh::BorshSchema))]
pub struct Meter {
    pub total: Bucket,
    pub flagged_self: Amount,
    pub flagged_seller_subaccount: Amount,
    pub flagged_round_trip: Amount,
    pub other: PayerStats,
    /// Payers with their own entry (at most `MAX_PAYERS`).
    pub tracked_payers: u64,
    /// Tracked payers with a counted payment.
    pub counted_payers: u64,
    /// Day list positions `[days_first, days_next)` are kept.
    pub days_first: u64,
    pub days_next: u64,
    /// Newest day with a payment.
    pub last_day: Option<u64>,
}

/// The `get_earnings` view (schema `earnings.v1`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(not(target_arch = "wasm32"), derive(schemars::JsonSchema))]
pub struct Earnings {
    pub schema: String,
    pub gross: Amount,
    pub counted: Amount,
    pub verified: Amount,
    pub flagged: Amount,
    pub flagged_self: Amount,
    pub flagged_seller_subaccount: Amount,
    pub flagged_round_trip: Amount,
    pub payments: u64,
    pub counted_payers: u64,
    pub fresh_payers: u64,
    pub counted_from_fresh_payers: Amount,
    pub tracked_payers: u64,
    pub other_payers_gross: Amount,
    /// Day rows kept (`get_earnings_days` pages through them).
    pub days_total: u64,
    /// The newest `EARNINGS_DAYS` day rows, oldest first.
    pub days: Vec<DayRow>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(not(target_arch = "wasm32"), derive(schemars::JsonSchema))]
pub struct DayRow {
    pub day: u64,
    pub gross: Amount,
    pub counted: Amount,
    pub verified: Amount,
    pub payments: u64,
}

impl DayRow {
    fn of(day: u64, b: &Bucket) -> Self {
        DayRow {
            day,
            gross: b.gross,
            counted: b.counted,
            verified: b.verified,
            payments: b.payments,
        }
    }
}

impl Meter {
    #[allow(clippy::too_many_arguments)]
    pub fn record(
        &mut self,
        st: &mut impl Store,
        payer: &str,
        amount: Amount,
        flags: Flags,
        verified: bool,
        now: Ts,
    ) {
        let counted = !flags.any();
        self.total.add(amount, counted, verified);
        if flags.self_pay {
            self.flagged_self += amount;
        } else if flags.seller_subaccount {
            self.flagged_seller_subaccount += amount;
        } else if flags.round_trip {
            self.flagged_round_trip += amount;
        }
        let today = now / DAY_NS;
        let mut row = if self.last_day == Some(today) {
            st.day(today).unwrap_or_default()
        } else {
            match st.day(today) {
                Some(r) => r,
                None => {
                    self.new_day(st, today);
                    DayStats::default()
                }
            }
        };
        if self.last_day.is_none_or(|d| today > d) {
            self.last_day = Some(today);
        }
        row.totals.add(amount, counted, verified);

        let existing = st.payer(payer);
        let tracked = existing.is_some()
            || ((self.tracked_payers as usize) < MAX_PAYERS && amount >= MIN_NEW_PAYER);
        if !tracked {
            let o = &mut self.other;
            o.totals.add(amount, counted, verified);
            o.last_ts = now;
            o.flags.self_pay |= flags.self_pay;
            o.flags.seller_subaccount |= flags.seller_subaccount;
            o.flags.round_trip |= flags.round_trip;
            st.put_day(today, &row);
            return;
        }
        let mut stats = existing.unwrap_or_else(|| {
            self.tracked_payers += 1;
            PayerStats {
                first_ts: now,
                ..Default::default()
            }
        });
        let first_day = stats.first_ts / DAY_NS;
        if counted {
            let newly = stats.totals.counted == 0;
            if newly {
                self.counted_payers += 1;
            }
            if first_day == today {
                row.new_counted_payers += newly as u64;
                row.counted_from_new += amount;
            } else if let Some(mut first) = st.day(first_day) {
                // The first day's row may have been dropped (older than
                // `MAX_DAYS`): such a payer is not fresh anyway.
                first.new_counted_payers += newly as u64;
                first.counted_from_new += amount;
                st.put_day(first_day, &first);
            }
        }
        stats.totals.add(amount, counted, verified);
        stats.last_ts = now;
        stats.flags.self_pay |= flags.self_pay;
        stats.flags.seller_subaccount |= flags.seller_subaccount;
        stats.flags.round_trip |= flags.round_trip;
        st.put_payer(payer, &stats);
        st.put_day(today, &row);
    }

    /// Append `day` to the day list; drop the oldest row past `MAX_DAYS`.
    fn new_day(&mut self, st: &mut impl Store, day: u64) {
        st.put_day_key(self.days_next, day);
        self.days_next += 1;
        if self.days_next - self.days_first > MAX_DAYS {
            if let Some(old) = st.day_key(self.days_first) {
                st.remove_day(old);
            }
            st.remove_day_key(self.days_first);
            self.days_first += 1;
        }
    }

    /// Day rows `[from, from + limit)` of the kept list, oldest first.
    pub fn days(&self, st: &impl Store, from: u64, limit: u64) -> Vec<DayRow> {
        let start = self.days_first.saturating_add(from);
        let end = start.saturating_add(limit).min(self.days_next);
        (start..end)
            .filter_map(|i| st.day_key(i))
            .filter_map(|d| st.day(d).map(|r| DayRow::of(d, &r.totals)))
            .collect()
    }

    pub fn days_total(&self) -> u64 {
        self.days_next - self.days_first
    }

    /// The `get_earnings` view. Reads at most `EARNINGS_DAYS` + 7 rows.
    /// "Fresh" = first paid in the last 7 UTC days (today included).
    pub fn earnings(&self, st: &impl Store, now: Ts) -> Earnings {
        let today = now / DAY_NS;
        let window = FRESH_WINDOW_NS / DAY_NS;
        let (mut fresh, mut fresh_counted) = (0u64, 0 as Amount);
        for d in today.saturating_sub(window - 1)..=today {
            if let Some(r) = st.day(d) {
                fresh += r.new_counted_payers;
                fresh_counted += r.counted_from_new;
            }
        }
        let total = self.days_total();
        Earnings {
            schema: "earnings.v1".into(),
            gross: self.total.gross,
            counted: self.total.counted,
            verified: self.total.verified,
            flagged: self.total.gross - self.total.counted,
            flagged_self: self.flagged_self,
            flagged_seller_subaccount: self.flagged_seller_subaccount,
            flagged_round_trip: self.flagged_round_trip,
            payments: self.total.payments,
            counted_payers: self.counted_payers,
            fresh_payers: fresh,
            counted_from_fresh_payers: fresh_counted,
            tracked_payers: self.tracked_payers,
            other_payers_gross: self.other.totals.gross,
            days_total: total,
            days: self.days(st, total.saturating_sub(EARNINGS_DAYS), EARNINGS_DAYS),
        }
    }
}
