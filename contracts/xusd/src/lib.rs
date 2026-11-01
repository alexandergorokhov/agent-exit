//! xUSD: a FAKE test dollar (NEP-141) for the Agent Exit demo.
//!
//! IT HAS NO VALUE. It exists so the demo can move "dollars" on testnet and in
//! tests without real money. Anyone can take up to `FAUCET_CAP` from the
//! faucet; the admin can mint for seeding the fixed demo world. The metadata
//! name says FAKE so no wallet can show it as a real stablecoin.

use near_contract_standards::fungible_token::metadata::{
    FungibleTokenMetadata, FungibleTokenMetadataProvider, FT_METADATA_SPEC,
};
use near_contract_standards::fungible_token::{
    FungibleToken, FungibleTokenCore, FungibleTokenResolver,
};
use near_contract_standards::storage_management::{
    StorageBalance, StorageBalanceBounds, StorageManagement,
};
use near_sdk::collections::LookupMap;
use near_sdk::json_types::U128;
use near_sdk::{
    env, near, require, AccountId, BorshStorageKey, NearToken, PanicOnDefault, PromiseOrValue,
};

/// 6 decimals, like USDC.
pub const ONE: u128 = 1_000_000;
/// Most any one account can ever take from the faucet: 10,000.00 xUSD.
pub const FAUCET_CAP: u128 = 10_000 * ONE;

#[derive(BorshStorageKey)]
#[near]
enum Key {
    Token,
    Faucet,
}

#[near(contract_state)]
#[derive(PanicOnDefault)]
pub struct Xusd {
    token: FungibleToken,
    admin: AccountId,
    faucet_taken: LookupMap<AccountId, u128>,
}

#[near]
impl Xusd {
    #[init]
    pub fn new(admin_id: AccountId) -> Self {
        Self {
            token: FungibleToken::new(Key::Token),
            admin: admin_id,
            faucet_taken: LookupMap::new(Key::Faucet),
        }
    }

    /// Take FAKE dollars for testing. Registers the caller if needed.
    /// Capped at `FAUCET_CAP` per account over its lifetime.
    pub fn faucet(&mut self, amount: U128) -> U128 {
        let who = env::predecessor_account_id();
        let taken = self.faucet_taken.get(&who).unwrap_or(0);
        let new_total = taken.checked_add(amount.0).expect("overflow");
        require!(new_total <= FAUCET_CAP, "FaucetCap");
        self.faucet_taken.insert(&who, &new_total);
        self.mint_to(&who, amount.0);
        U128(FAUCET_CAP - new_total)
    }

    /// Admin seeds demo accounts. Testnet only; the token is fake.
    pub fn mint(&mut self, account_id: AccountId, amount: U128) {
        require!(env::predecessor_account_id() == self.admin, "NotAdmin");
        self.mint_to(&account_id, amount.0);
    }

    pub fn faucet_left(&self, account_id: AccountId) -> U128 {
        U128(FAUCET_CAP - self.faucet_taken.get(&account_id).unwrap_or(0))
    }
}

impl Xusd {
    fn mint_to(&mut self, who: &AccountId, amount: u128) {
        if !self.token.accounts.contains_key(who) {
            self.token.internal_register_account(who);
        }
        self.token.internal_deposit(who, amount);
        near_contract_standards::fungible_token::events::FtMint {
            owner_id: who,
            amount: U128(amount),
            memo: Some("FAKE test dollars"),
        }
        .emit();
    }
}

#[near]
impl FungibleTokenCore for Xusd {
    #[payable]
    fn ft_transfer(&mut self, receiver_id: AccountId, amount: U128, memo: Option<String>) {
        self.token.ft_transfer(receiver_id, amount, memo)
    }

    #[payable]
    fn ft_transfer_call(
        &mut self,
        receiver_id: AccountId,
        amount: U128,
        memo: Option<String>,
        msg: String,
    ) -> PromiseOrValue<U128> {
        self.token.ft_transfer_call(receiver_id, amount, memo, msg)
    }

    fn ft_total_supply(&self) -> U128 {
        self.token.ft_total_supply()
    }

    fn ft_balance_of(&self, account_id: AccountId) -> U128 {
        self.token.ft_balance_of(account_id)
    }
}

#[near]
impl FungibleTokenResolver for Xusd {
    #[private]
    fn ft_resolve_transfer(
        &mut self,
        sender_id: AccountId,
        receiver_id: AccountId,
        amount: U128,
    ) -> U128 {
        let (used, burned) =
            self.token
                .internal_ft_resolve_transfer(&sender_id, receiver_id, amount);
        if burned > 0 {
            near_contract_standards::fungible_token::events::FtBurn {
                owner_id: &sender_id,
                amount: U128(burned),
                memo: Some("refund to closed account"),
            }
            .emit();
        }
        used.into()
    }
}

#[near]
impl StorageManagement for Xusd {
    #[payable]
    fn storage_deposit(
        &mut self,
        account_id: Option<AccountId>,
        registration_only: Option<bool>,
    ) -> StorageBalance {
        self.token.storage_deposit(account_id, registration_only)
    }

    #[payable]
    fn storage_withdraw(&mut self, amount: Option<NearToken>) -> StorageBalance {
        self.token.storage_withdraw(amount)
    }

    #[payable]
    fn storage_unregister(&mut self, force: Option<bool>) -> bool {
        self.token.storage_unregister(force)
    }

    fn storage_balance_bounds(&self) -> StorageBalanceBounds {
        self.token.storage_balance_bounds()
    }

    fn storage_balance_of(&self, account_id: AccountId) -> Option<StorageBalance> {
        self.token.storage_balance_of(account_id)
    }
}

#[near]
impl FungibleTokenMetadataProvider for Xusd {
    fn ft_metadata(&self) -> FungibleTokenMetadata {
        FungibleTokenMetadata {
            spec: FT_METADATA_SPEC.to_string(),
            name: "FAKE Test USD (no value, Agent Exit demo)".to_string(),
            symbol: "xUSD-FAKE".to_string(),
            icon: None,
            reference: None,
            reference_hash: None,
            decimals: 6,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use near_sdk::test_utils::{accounts, VMContextBuilder};
    use near_sdk::testing_env;

    fn ctx(who: AccountId) {
        testing_env!(VMContextBuilder::new()
            .predecessor_account_id(who)
            .attached_deposit(NearToken::from_yoctonear(1))
            .build());
    }

    #[test]
    fn metadata_says_fake() {
        ctx(accounts(0));
        let t = Xusd::new(accounts(0));
        let m = t.ft_metadata();
        assert!(m.name.contains("FAKE"));
        assert!(m.symbol.contains("FAKE"));
        assert_eq!(m.decimals, 6);
    }

    #[test]
    fn faucet_is_capped() {
        ctx(accounts(0));
        let mut t = Xusd::new(accounts(0));
        ctx(accounts(1));
        assert_eq!(t.faucet(U128(4_000 * ONE)).0, 6_000 * ONE);
        assert_eq!(t.faucet(U128(6_000 * ONE)).0, 0);
        assert_eq!(t.ft_balance_of(accounts(1)).0, FAUCET_CAP);
    }

    #[test]
    #[should_panic(expected = "FaucetCap")]
    fn faucet_over_cap_panics() {
        ctx(accounts(0));
        let mut t = Xusd::new(accounts(0));
        ctx(accounts(1));
        t.faucet(U128(FAUCET_CAP + 1));
    }

    #[test]
    #[should_panic(expected = "NotAdmin")]
    fn only_admin_mints() {
        ctx(accounts(0));
        let mut t = Xusd::new(accounts(0));
        ctx(accounts(2));
        t.mint(accounts(2), U128(1));
    }

    #[test]
    fn admin_mints_and_transfers() {
        ctx(accounts(0));
        let mut t = Xusd::new(accounts(0));
        t.mint(accounts(1), U128(500 * ONE));
        t.mint(accounts(2), U128(0));
        ctx(accounts(1));
        t.ft_transfer(accounts(2), U128(200 * ONE), None);
        assert_eq!(t.ft_balance_of(accounts(1)).0, 300 * ONE);
        assert_eq!(t.ft_balance_of(accounts(2)).0, 200 * ONE);
        assert_eq!(t.ft_total_supply().0, 500 * ONE);
    }
}
