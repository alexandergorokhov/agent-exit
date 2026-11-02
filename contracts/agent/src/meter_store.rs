//! The contract's `Store` (A47): payer rows, day rows, payout destinations,
//! claimable balances, past owners and the sale history, each entry under
//! its own storage key, read and written one at a time. The handle itself is
//! empty, so the state blob holds only the meter's aggregates and loading
//! the contract costs the same with 1 payer or 1,000.

use agent_core::meter::{DayStats, PayerStats, Store};
use agent_core::{Amount, Sale};
use near_sdk::borsh::{self, BorshDeserialize, BorshSerialize};
use near_sdk::env;
use near_sdk::serde::{Deserialize, Serialize};

const PAYER: &[u8] = b"mp";
const DAY: &[u8] = b"md";
const DAY_KEY: &[u8] = b"mk";
const PAID_TO: &[u8] = b"pt";
const CLAIMABLE: &[u8] = b"cl";
const PAST_OWNER: &[u8] = b"po";
const SALE: &[u8] = b"sh";

#[derive(
    Debug,
    Clone,
    Copy,
    Default,
    PartialEq,
    Eq,
    BorshSerialize,
    BorshDeserialize,
    Serialize,
    Deserialize,
)]
#[borsh(crate = "near_sdk::borsh")]
#[serde(crate = "near_sdk::serde")]
#[cfg_attr(not(target_arch = "wasm32"), derive(near_sdk::borsh::BorshSchema))]
pub struct NearStore;

fn key(prefix: &[u8], id: &[u8]) -> Vec<u8> {
    let mut k = Vec::with_capacity(prefix.len() + id.len());
    k.extend_from_slice(prefix);
    k.extend_from_slice(id);
    k
}

fn read<T: BorshDeserialize>(k: &[u8]) -> Option<T> {
    env::storage_read(k)
        .map(|v| T::try_from_slice(&v).unwrap_or_else(|_| env::panic_str("BadMeterRow")))
}

fn write<T: BorshSerialize>(k: &[u8], v: &T) {
    env::storage_write(k, &borsh::to_vec(v).expect("borsh"));
}

impl Store for NearStore {
    fn payer(&self, account: &str) -> Option<PayerStats> {
        read(&key(PAYER, account.as_bytes()))
    }
    fn put_payer(&mut self, account: &str, stats: &PayerStats) {
        write(&key(PAYER, account.as_bytes()), stats)
    }
    fn remove_payer(&mut self, account: &str) {
        env::storage_remove(&key(PAYER, account.as_bytes()));
    }
    fn day(&self, day: u64) -> Option<DayStats> {
        read(&key(DAY, &day.to_be_bytes()))
    }
    fn put_day(&mut self, day: u64, stats: &DayStats) {
        write(&key(DAY, &day.to_be_bytes()), stats)
    }
    fn remove_day(&mut self, day: u64) {
        env::storage_remove(&key(DAY, &day.to_be_bytes()));
    }
    fn day_key(&self, index: u64) -> Option<u64> {
        read(&key(DAY_KEY, &index.to_be_bytes()))
    }
    fn put_day_key(&mut self, index: u64, day: u64) {
        write(&key(DAY_KEY, &index.to_be_bytes()), &day)
    }
    fn remove_day_key(&mut self, index: u64) {
        env::storage_remove(&key(DAY_KEY, &index.to_be_bytes()));
    }
    fn paid_to(&self, account: &str) -> Option<u64> {
        read(&key(PAID_TO, account.as_bytes()))
    }
    fn put_paid_to(&mut self, account: &str, epoch: u64) {
        write(&key(PAID_TO, account.as_bytes()), &epoch)
    }
    fn claimable(&self, account: &str) -> Amount {
        read(&key(CLAIMABLE, account.as_bytes())).unwrap_or(0)
    }
    fn put_claimable(&mut self, account: &str, amount: Amount) {
        let k = key(CLAIMABLE, account.as_bytes());
        if amount == 0 {
            env::storage_remove(&k);
        } else {
            write(&k, &amount)
        }
    }
    fn past_owner(&self, account: &str) -> bool {
        env::storage_has_key(&key(PAST_OWNER, account.as_bytes()))
    }
    fn put_past_owner(&mut self, account: &str) {
        env::storage_write(&key(PAST_OWNER, account.as_bytes()), &[]);
    }
    fn sale(&self, id: u64) -> Option<Sale> {
        read(&key(SALE, &id.to_be_bytes()))
    }
    fn put_sale(&mut self, sale: &Sale) {
        write(&key(SALE, &sale.id.to_be_bytes()), sale)
    }
}
