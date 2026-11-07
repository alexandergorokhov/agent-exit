//! Born-clean agent factory.
//!
//! `create_agent(name)` creates `name.<factory>` in ONE batch: create the
//! account, fund it, deploy the published Agent Exit code (stored here once,
//! checked against its code hash) and call `new(owner)`. No access key is
//! ever added, so the account starts with no key at all, and the agent
//! contract itself only ever adds function-call keys. Together:
//!
//! - no hidden full-access key can exist on a factory-born agent (N4), at
//!   any time, not just at the moment of a scan;
//! - no state can be written before the published code ran (N12): the
//!   account did not exist before this batch, and `new` is its first call;
//! - the code cannot change: there is no key to deploy with, and the agent
//!   contract has no upgrade method.
//!
//! (A NEAR global contract would keep one copy of the code for every agent;
//! on testnet it costs ten times the storage of one copy, so the factory
//! stores the code once and deploys it into each agent.)
//!
//! The registry (`get_agent`, `list_agents`) records each birth: owner at
//! birth, code hash, block. A buyer's scan checks it (`born_clean`).
//! A parent account has no power over its sub-accounts on NEAR, so the
//! factory cannot touch an agent after creating it.
use near_sdk::json_types::{Base58CryptoHash, U128, U64};
use near_sdk::store::{LookupMap, Vector};
use near_sdk::{
    env, near, require, AccountId, CryptoHash, Gas, NearToken, PanicOnDefault, Promise,
    PromiseOrValue,
};
use serde_json::json;

/// Least NEAR a new agent starts with: its code (~5.3 for 550 KB), the
/// growth reserve its contract demands (about 1.83) and the 1 NEAR reserve.
pub const MIN_AGENT_NEAR: NearToken = NearToken::from_near(9);
const CODE_KEY: &[u8] = b"code";
const GAS_INIT: Gas = Gas::from_tgas(60);
const GAS_ON_CREATED: Gas = Gas::from_tgas(15);
pub const MAX_LIST: u32 = 100;

#[near(serializers = [json, borsh])]
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum BirthState {
    Creating,
    Born,
}

/// One birth record (schema `agent-birth.v1`).
#[near(serializers = [json, borsh])]
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Birth {
    pub agent: AccountId,
    pub owner_at_birth: AccountId,
    /// Who attached the NEAR (refunded if the creation fails).
    pub paid_by: AccountId,
    pub code_hash: Base58CryptoHash,
    pub state: BirthState,
    pub created_at_ns: U64,
    pub block_height: U64,
    pub deposit: U128,
}

#[near(contract_state)]
#[derive(PanicOnDefault)]
pub struct Factory {
    code_hash: CryptoHash,
    births: LookupMap<AccountId, Birth>,
    order: Vector<AccountId>,
}

fn event(name: &str, data: serde_json::Value) {
    env::log_str(&format!(
        "EVENT_JSON:{}",
        json!({"standard": "agent_exit_factory", "version": "1.0.0", "event": name, "data": [data]})
    ));
}

/// `name` becomes `name.<factory>`: lowercase letters, digits, `-` and `_`,
/// 2 to 32 characters, no dots (one level, so the factory is the only parent).
pub fn valid_name(name: &str) -> bool {
    (2..=32).contains(&name.len())
        && name
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-' || b == b'_')
        && !name.starts_with(['-', '_'])
        && !name.ends_with(['-', '_'])
}

#[near]
impl Factory {
    /// `code_hash`: the published Agent Exit wasm (`contracts/res/SHA256SUMS`
    /// gives its sha256; base58 of it is the code hash NEAR reports). The
    /// bytes follow with `store_code`, and only bytes with this hash are taken.
    #[init]
    pub fn new(code_hash: Base58CryptoHash) -> Self {
        Self {
            code_hash: code_hash.into(),
            births: LookupMap::new(b"b"),
            order: Vector::new(b"o"),
        }
    }

    /// The factory account uploads the wasm (raw bytes as the call's input).
    /// Refused unless sha256(bytes) is the hash fixed at init, so the code
    /// every agent gets is the published build.
    #[private]
    pub fn store_code(&mut self) {
        require!(
            env::predecessor_account_id() == env::current_account_id(),
            "Method store_code is private"
        );
        let code = env::input().unwrap_or_default();
        require!(
            env::sha256_array(&code) == self.code_hash,
            "CodeHashMismatch: not the published build"
        );
        env::storage_write(CODE_KEY, &code);
        event(
            "code_stored",
            json!({"code_hash": Base58CryptoHash::from(self.code_hash), "bytes": code.len()}),
        );
    }

    pub fn has_code(&self) -> bool {
        env::storage_has_key(CODE_KEY)
    }

    /// Create `name.<factory>`, born clean, owned by `owner_id` (default: the
    /// caller, who pays). Attach at least
    /// `MIN_AGENT_NEAR`; all of it goes to the agent. Refunded in full if the
    /// creation fails.
    #[payable]
    pub fn create_agent(
        &mut self,
        name: String,
        owner_id: Option<AccountId>,
        base_address: Option<String>,
    ) -> Promise {
        require!(valid_name(&name), "BadName: 2-32 of a-z 0-9 - _, no dots");
        let deposit = env::attached_deposit();
        require!(
            deposit >= MIN_AGENT_NEAR,
            "DepositTooSmall: attach at least 9 NEAR"
        );
        let code = env::storage_read(CODE_KEY)
            .unwrap_or_else(|| env::panic_str("NoCode: store_code first"));
        let agent: AccountId = format!("{name}.{}", env::current_account_id())
            .parse()
            .unwrap_or_else(|_| env::panic_str("BadName"));
        require!(!self.births.contains_key(&agent), "NameTaken");
        let payer = env::predecessor_account_id();
        let owner = owner_id.unwrap_or_else(|| payer.clone());
        let birth = Birth {
            agent: agent.clone(),
            owner_at_birth: owner.clone(),
            paid_by: payer,
            code_hash: Base58CryptoHash::from(self.code_hash),
            state: BirthState::Creating,
            created_at_ns: U64(env::block_timestamp()),
            block_height: U64(env::block_height()),
            deposit: U128(deposit.as_yoctonear()),
        };
        self.births.insert(agent.clone(), birth);
        let args = json!({"owner_id": owner, "base_address": base_address});
        Promise::new(agent.clone())
            .create_account()
            .transfer(deposit)
            .deploy_contract(code)
            .function_call(
                "new".to_string(),
                args.to_string().into_bytes(),
                NearToken::from_yoctonear(0),
                GAS_INIT,
            )
            .then(
                Self::ext(env::current_account_id())
                    .with_static_gas(GAS_ON_CREATED)
                    .on_created(agent),
            )
    }

    /// After the batch. Success: the birth is recorded. Failure: nothing was
    /// created (a batch is all or nothing), the runtime returned the NEAR to
    /// the factory, and it goes back to the caller.
    #[private]
    pub fn on_created(&mut self, agent: AccountId) -> PromiseOrValue<bool> {
        // #[private] checks this in the wasm wrapper; checked here too so the
        // unit tests see it.
        require!(
            env::predecessor_account_id() == env::current_account_id(),
            "Method on_created is private"
        );
        let mut b = self
            .births
            .get(&agent)
            .cloned()
            .unwrap_or_else(|| env::panic_str("unknown agent"));
        require!(b.state == BirthState::Creating, "not being created");
        if env::promise_result_checked(0, 1_024).is_ok() {
            b.state = BirthState::Born;
            self.order.push(agent.clone());
            event(
                "agent_born",
                json!({"agent": agent, "owner": b.owner_at_birth, "code_hash": b.code_hash,
                       "access_keys_added": 0, "deposit": b.deposit}),
            );
            self.births.insert(agent, b);
            PromiseOrValue::Value(true)
        } else {
            self.births.remove(&agent);
            event(
                "agent_birth_failed",
                json!({"agent": agent, "refund_to": b.paid_by, "amount": b.deposit}),
            );
            Promise::new(b.paid_by)
                .transfer(NearToken::from_yoctonear(b.deposit.0))
                .into()
        }
    }

    /// The birth record of `account_id`, if the factory created it.
    pub fn get_agent(&self, account_id: AccountId) -> Option<Birth> {
        self.births.get(&account_id).cloned()
    }

    /// Agents born here, oldest first.
    pub fn list_agents(&self, from_index: Option<u32>, limit: Option<u32>) -> Vec<Birth> {
        let from = from_index.unwrap_or(0);
        let n = limit.unwrap_or(MAX_LIST).min(MAX_LIST);
        (from..self.order.len().min(from.saturating_add(n)))
            .filter_map(|i| self.order.get(i))
            .filter_map(|a| self.births.get(a).cloned())
            .collect()
    }

    pub fn get_code_hash(&self) -> Base58CryptoHash {
        Base58CryptoHash::from(self.code_hash)
    }

    pub fn total_agents(&self) -> u32 {
        self.order.len()
    }
}

#[cfg(test)]
mod tests;
