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

#[derive(
    Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, BorshSerialize, BorshDeserialize,
)]
#[cfg_attr(not(target_arch = "wasm32"), derive(borsh::BorshSchema))]
pub struct Meter {
    pub total: Bucket,
    pub flagged_self: Amount,
    pub flagged_seller_subaccount: Amount,
    pub flagged_round_trip: Amount,
    pub payers: BTreeMap<Account, PayerStats>,
    pub other: PayerStats,
    /// Keyed by UTC day number (timestamp / 86400 s).
    pub days: BTreeMap<u64, Bucket>,
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

impl Meter {
    pub fn record(&mut self, payer: &str, amount: Amount, flags: Flags, verified: bool, now: Ts) {
        let counted = !flags.any();
        self.total.add(amount, counted, verified);
        if flags.self_pay {
            self.flagged_self += amount;
        } else if flags.seller_subaccount {
            self.flagged_seller_subaccount += amount;
        } else if flags.round_trip {
            self.flagged_round_trip += amount;
        }
        self.days
            .entry(now / DAY_NS)
            .or_default()
            .add(amount, counted, verified);
        let stats = if self.payers.contains_key(payer) || self.payers.len() < MAX_PAYERS {
            self.payers
                .entry(payer.to_string())
                .or_insert_with(|| PayerStats {
                    first_ts: now,
                    ..Default::default()
                })
        } else {
            &mut self.other
        };
        stats.totals.add(amount, counted, verified);
        stats.last_ts = now;
        stats.flags.self_pay |= flags.self_pay;
        stats.flags.seller_subaccount |= flags.seller_subaccount;
        stats.flags.round_trip |= flags.round_trip;
    }

    pub fn earnings(&self, now: Ts) -> Earnings {
        let counted_payers: Vec<&PayerStats> = self
            .payers
            .values()
            .filter(|p| p.totals.counted > 0)
            .collect();
        let fresh: Vec<&&PayerStats> = counted_payers
            .iter()
            .filter(|p| now.saturating_sub(p.first_ts) < FRESH_WINDOW_NS)
            .collect();
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
            counted_payers: counted_payers.len() as u64,
            fresh_payers: fresh.len() as u64,
            counted_from_fresh_payers: fresh.iter().map(|p| p.totals.counted).sum(),
            tracked_payers: self.payers.len() as u64,
            other_payers_gross: self.other.totals.gross,
            days: self
                .days
                .iter()
                .map(|(d, b)| DayRow {
                    day: *d,
                    gross: b.gross,
                    counted: b.counted,
                    verified: b.verified,
                    payments: b.payments,
                })
                .collect(),
        }
    }
}
