//! Shares of future earnings (PLAN-shares.md, SH1-SH11, threats T1-T17).
//!
//! The owner offers a slice: `bps` of every payment the agent receives, until
//! `end`, for `price`. Investors buy parts of the slice. Each payment into the
//! agent credits each holder's part of the **gross** payment to the holder's
//! claimable balance inside `pay()` (T1, T3), so the owner never holds it.
//!
//! Price escrow (SH11, T2): half of what an investor pays goes to the owner
//! now; the other half waits in the holding and is released to the seller of
//! the slice only as the holder is paid. At `end`, whatever is still held goes
//! back to the investor. An owner who takes the money and routes customers
//! elsewhere keeps at most half.

use crate::{Account, Amount, Ts, DAY_NS};
use borsh::{BorshDeserialize, BorshSerialize};
use serde::{Deserialize, Serialize};

/// Open slices at most total this share of every payment (SH1): the owner
/// keeps the majority.
pub const MAX_SHARES_BPS: u16 = 4_900;
/// Smallest slice the owner may offer: 1% of earnings.
pub const MIN_OFFER_BPS: u16 = 100;
/// Investors across all open slices (SH5): bounds the work `pay()` does.
pub const MAX_HOLDERS: usize = 16;
/// Open slices at a time.
pub const MAX_OPEN_OFFERS: usize = 2;
/// Smallest part an investor may buy: a tenth of the slice, or what is left
/// (T10). Filling every holder slot then costs the whole slice.
pub const MIN_PART_DIV: u16 = 10;
/// Slice length bounds.
pub const MIN_TERM_NS: u64 = DAY_NS;
pub const MAX_TERM_NS: u64 = 2 * 365 * DAY_NS;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, BorshSerialize, BorshDeserialize)]
#[cfg_attr(not(target_arch = "wasm32"), derive(borsh::BorshSchema))]
pub struct Holding {
    pub account: Account,
    /// Share of every gross payment this holder receives.
    pub bps: u16,
    /// What the holder paid.
    pub paid_in: Amount,
    /// Price escrow still held for this holder (SH11).
    pub escrow: Amount,
    /// Earnings credited to the holder so far.
    pub earned: Amount,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, BorshSerialize, BorshDeserialize)]
#[cfg_attr(not(target_arch = "wasm32"), derive(borsh::BorshSchema))]
pub struct ShareOffer {
    pub id: u64,
    /// The owner who offered it; escrow releases go to this account (T9).
    pub seller: Account,
    /// Slice of every gross payment, in basis points.
    pub bps: u16,
    /// Price of the whole slice.
    pub price: Amount,
    pub created: Ts,
    pub end: Ts,
    /// Parts sold so far (sum of the holders' bps).
    pub sold_bps: u16,
    pub holders: Vec<Holding>,
}

/// Terms an investor saw; `buy_share` refuses on any difference (T6).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct ShareTerms {
    pub offer_id: u64,
    pub bps: u16,
    pub price: Amount,
    pub end: Ts,
}

impl ShareOffer {
    pub fn terms(&self) -> ShareTerms {
        ShareTerms {
            offer_id: self.id,
            bps: self.bps,
            price: self.price,
            end: self.end,
        }
    }

    pub fn active(&self, now: Ts) -> bool {
        now < self.end
    }

    /// Price of `part` basis points of this slice, rounded up so parts never
    /// sum to less than the slice price.
    pub fn cost(&self, part: u16) -> Amount {
        let n = self
            .price
            .checked_mul(part as u128)
            .expect("overflow in share cost");
        n.div_ceil(self.bps as u128)
    }

    /// Smallest part that may be bought now (T10).
    pub fn min_part(&self) -> u16 {
        (self.bps / MIN_PART_DIV)
            .max(1)
            .min(self.bps - self.sold_bps)
    }

    pub fn escrow(&self) -> Amount {
        self.holders.iter().map(|h| h.escrow).sum()
    }
}

/// Split what an investor pays into (to the seller now, price escrow). The
/// seller's half rounds up; the two add up exactly.
pub fn split_cost(cost: Amount) -> (Amount, Amount) {
    let escrow = cost / 2;
    (cost - escrow, escrow)
}

/// One holder's part of a gross payment, rounded down (rounding stays with
/// the owner, T13).
pub fn holder_cut(payment: Amount, bps: u16) -> Amount {
    crate::money::mul_bps(payment, bps)
}
