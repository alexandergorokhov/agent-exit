//! Spending policy (feature 12): the owner sets limits, the contract enforces
//! them. The agent's runner key can pay out of the agent's treasury only to
//! allowed payees and only up to the daily cap; every incoming payment is
//! split between the owner's revenue and the treasury.
//!
//! The limits belong to the deed: after a sale or deed transfer the new owner
//! controls them and the old owner gets `NotOwner`. Spending is counted per
//! (owner epoch, UTC day), so a seller who burns the cap just before the sale
//! does not reduce the buyer's cap (E18, A15).

use crate::{Account, Amount, Error, Res, USD};
use borsh::{BorshDeserialize, BorshSerialize};
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;

/// Highest accepted cap or price floor (1,000,000.00 test dollars). Anything
/// above is treated as a typo (E33).
pub const MAX_LIMIT: Amount = 1_000_000 * USD;
pub const MAX_PAYEES: usize = 16;

/// Which limit a refused payment hit. Printed as `PolicyViolation:<name>`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Policy {
    /// The payee is not in `allowed_payees`.
    Payee,
    /// Spent today + this payment > `daily_cap`.
    DailyCap,
    /// An incoming job payment below `price_floor`.
    PriceFloor,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, BorshSerialize, BorshDeserialize)]
#[cfg_attr(not(target_arch = "wasm32"), derive(borsh::BorshSchema))]
pub struct Limits {
    /// Smallest accepted incoming payment (0 = none).
    pub price_floor: Amount,
    /// Most the runner may pay out of the treasury per UTC day (0 = nothing).
    pub daily_cap: Amount,
    /// The only accounts the runner may pay.
    pub allowed_payees: BTreeSet<Account>,
    /// Share of each incoming payment that goes to the owner's revenue.
    pub owner_bps: u16,
    /// Share of each incoming payment that goes to the agent's treasury.
    pub treasury_bps: u16,
}

impl Default for Limits {
    /// Before the owner sets limits: everything to the owner, the runner may
    /// pay nobody.
    fn default() -> Self {
        Limits {
            price_floor: 0,
            daily_cap: 0,
            allowed_payees: BTreeSet::new(),
            owner_bps: 10_000,
            treasury_bps: 0,
        }
    }
}

impl Limits {
    /// Sanity checks (E33): the split adds up to 100%, caps are not absurd,
    /// the agent cannot pay itself, the payee list is short.
    pub fn check(&self, agent: &str) -> Res<()> {
        let ok = self.owner_bps as u32 + self.treasury_bps as u32 == 10_000
            && self.daily_cap <= MAX_LIMIT
            && self.price_floor <= MAX_LIMIT
            && self.allowed_payees.len() <= MAX_PAYEES
            && !self.allowed_payees.contains(agent)
            && self.allowed_payees.iter().all(|p| !p.is_empty());
        if ok {
            Ok(())
        } else {
            Err(Error::BadLimits)
        }
    }
}

/// Spending in the current (owner epoch, day). A different epoch or day means
/// nothing was spent yet.
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
pub struct Spent {
    pub owner_epoch: u64,
    pub day: u64,
    pub amount: Amount,
}

impl Spent {
    pub fn today(&self, owner_epoch: u64, day: u64) -> Amount {
        if self.owner_epoch == owner_epoch && self.day == day {
            self.amount
        } else {
            0
        }
    }
}
