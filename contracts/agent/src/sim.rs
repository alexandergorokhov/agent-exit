//! In-process simulator: drives the real contract code in near-sdk's mocked
//! runtime. Promises do not execute here, so the simulator calls callbacks by
//! hand with the promise result a test chooses (success or failure). Used by
//! the unit tests and by `tools/verify` (the offline rerun kit).
//!
//! Network-level facts (a deleted key failing with `AccessKeyNotFound`) need
//! near-sandbox or testnet; the simulator checks the actions the contract
//! emits instead.

use crate::*;
use near_sdk::test_utils::{get_created_receipts, get_logs, VMContextBuilder};
use near_sdk::{CurveType, PromiseResult, RuntimeFeesConfig};

pub const USD: u128 = agent_core::USD;
/// 2026-01-01T00:00:00Z.
pub const T0: u64 = 1_767_225_600 * agent_core::NANOS_PER_SEC;
pub const DAY: u64 = agent_core::DAY_NS;

pub const AGENT: &str = "translator.testnet";
pub const CHEAT: &str = "cheat.testnet";
/// The pinned payment token (A26).
pub const TOKEN: &str = crate::PAYMENT_TOKEN;
pub const ALICE: &str = "alice.testnet";
pub const X_ALICE: &str = "x.alice.testnet";
pub const BOB: &str = "bob.testnet";
pub const CAROL: &str = "carol.testnet";
pub const MALLORY: &str = "mallory.testnet";
/// Registry admin: edits the NEAR AI signer list only.
pub const ADMIN: &str = crate::REGISTRY_ADMIN;
pub const PAYERS: [&str; 6] = [
    "p1.testnet",
    "p2.testnet",
    "p3.testnet",
    "p4.testnet",
    "p5.testnet",
    "p6.testnet",
];
pub const CODE_HASH: &str = "sha256:fixture-code-hash";
/// Another agent the translator buys sub-tasks from (feature 12) and that
/// later buys the translator through MCP (S14).
pub const SUMMARIZER: &str = "summarizer.testnet";

/// Deterministic ed25519 public key for tests: 32 bytes of `n`.
pub fn key(n: u8) -> PublicKey {
    PublicKey::from_parts(CurveType::ED25519, vec![n; 32]).expect("valid key")
}
pub fn key_str(n: u8) -> String {
    String::from(&key(n))
}

pub const ALICE_RUNNER: u8 = 1;
pub const BOB_RUNNER: u8 = 2;
pub const CAROL_RUNNER: u8 = 3;
pub const HIDDEN_KEY: u8 = 66;

pub fn acc(s: &str) -> AccountId {
    s.parse().expect("valid account")
}

/// One mocked call's environment.
#[derive(Clone)]
pub struct Call {
    pub predecessor: String,
    pub signer: String,
    pub signer_pk: PublicKey,
    pub deposit: NearToken,
    pub gas: Gas,
    /// One promise result for a callback: `Some(true)` success, `Some(false)` failure.
    pub result: Option<bool>,
    /// The success value of that promise (default empty), e.g. the MPC
    /// signer's answer for `on_base_signed`.
    pub result_data: Vec<u8>,
    /// Account storage in bytes (`env::storage_usage`); the mocked runtime
    /// never writes the contract state, so tests set it.
    pub storage: u64,
}

pub struct Sim {
    /// Which storage map is this simulator's (see `STORES`).
    id: u64,
    pub c: AgentExit,
    pub agent: String,
    pub now: u64,
    pub balance: NearToken,
}

impl Sim {
    /// Deploy + init on `agent` with `owner` as the first deed holder.
    pub fn new(agent: &str, owner: &str) -> Self {
        let mut s = Sim {
            id: NEXT_ID.with(|n| {
                let id = n.get();
                n.set(id + 1);
                id
            }),
            c: unsafe_placeholder(),
            agent: agent.into(),
            now: T0,
            balance: NearToken::from_near(10),
        };
        s.env(&s.call(agent));
        // The agent's Base address under the offline TEST MPC signer.
        let base =
            base_wallet::MockMpc::default().derive_address(agent, agent_core::base::BASE_PATH);
        s.c = AgentExit::new(acc(owner), Some(base));
        // The registry admin allowlists the LOCAL TEST signer.
        s.by(ADMIN).add_signer(test_signer());
        s
    }

    pub fn call(&self, who: &str) -> Call {
        Call {
            predecessor: who.into(),
            signer: who.into(),
            signer_pk: key(200),
            deposit: NearToken::from_yoctonear(0),
            gas: Gas::from_tgas(300),
            result: None,
            result_data: vec![],
            storage: 0,
        }
    }

    pub fn env(&self, c: &Call) {
        // Keep what the outgoing simulator's last call left (beyond the
        // storage that call was given).
        let used = near_sdk::env::storage_usage().saturating_sub(LAST_BASE.get());
        USAGE.with(|u| {
            u.borrow_mut().insert(CURRENT.get(), used);
        });
        LAST_BASE.set(c.storage);
        let ctx = VMContextBuilder::new()
            .current_account_id(acc(&self.agent))
            .predecessor_account_id(acc(&c.predecessor))
            .signer_account_id(acc(&c.signer))
            .signer_account_pk(c.signer_pk.clone())
            .attached_deposit(c.deposit)
            .prepaid_gas(c.gas)
            .block_timestamp(self.now)
            .account_balance(self.balance)
            .storage_usage(
                c.storage + USAGE.with(|u| u.borrow().get(&self.id).copied().unwrap_or_default()),
            )
            .build();
        let current = near_sdk::mock::with_mocked_blockchain(|b| b.take_storage());
        let mine = STORES.with(|st| {
            let mut st = st.borrow_mut();
            st.insert(CURRENT.get(), current);
            CURRENT.set(self.id);
            st.remove(&self.id).unwrap_or_default()
        });
        near_sdk::env::set_blockchain_interface(near_sdk::MockedBlockchain::new(
            ctx,
            near_sdk::test_vm_config(),
            RuntimeFeesConfig::test(),
            c.result
                .map(|ok| {
                    vec![if ok {
                        PromiseResult::Successful(c.result_data.clone())
                    } else {
                        PromiseResult::Failed
                    }]
                })
                .unwrap_or_default(),
            mine,
            Default::default(),
            None,
        ));
    }

    /// Set up the environment for a plain call by `who`.
    pub fn by(&mut self, who: &str) -> &mut AgentExit {
        let c = self.call(who);
        self.env(&c);
        &mut self.c
    }

    /// Same, with 1 yoctoNEAR attached.
    pub fn by_1y(&mut self, who: &str) -> &mut AgentExit {
        let mut c = self.call(who);
        c.deposit = NearToken::from_yoctonear(1);
        self.env(&c);
        &mut self.c
    }

    /// A callback on the agent itself with one promise result.
    pub fn callback(&mut self, ok: bool) -> &mut AgentExit {
        let mut c = self.call(&self.agent.clone());
        c.result = Some(ok);
        self.env(&c);
        &mut self.c
    }

    /// `on_runner_key` for the latest rotation batch (its nonce, A44).
    pub fn runner_key_cb(
        &mut self,
        ok: bool,
        old: Vec<String>,
        deleted: Vec<String>,
        new_key: String,
    ) -> bool {
        let batch = self.c.m.key_batch;
        self.callback(ok)
            .on_runner_key(old, deleted, new_key, batch)
    }

    /// A callback whose promise succeeded with `data` (the MPC answer).
    pub fn callback_data(&mut self, data: &Value) -> &mut AgentExit {
        let mut c = self.call(&self.agent.clone());
        c.result = Some(true);
        c.result_data = data.to_string().into_bytes();
        self.env(&c);
        &mut self.c
    }

    /// A self-call signed by `pk` (a runner call, or a hidden key).
    pub fn runner(&mut self, pk: PublicKey) -> &mut AgentExit {
        let mut c = self.call(&self.agent.clone());
        c.signer_pk = pk;
        self.env(&c);
        &mut self.c
    }

    /// `token.ft_transfer_call(agent, amount, msg)` by `payer`, arriving as
    /// `ft_on_transfer`. Returns the unused amount if the contract answered
    /// with a value, or `None` if it returned a promise (a buy in flight).
    pub fn ft(&mut self, payer: &str, amount: u128, msg: &str, gas_tgas: u64) -> Option<u128> {
        let mut c = self.call(TOKEN);
        c.signer = payer.into();
        c.gas = Gas::from_tgas(gas_tgas);
        self.env(&c);
        match self.c.ft_on_transfer(acc(payer), U128(amount), msg.into()) {
            PromiseOrValue::Value(v) => Some(v.0),
            PromiseOrValue::Promise(_) => None,
        }
    }

    pub fn pay(&mut self, payer: &str, amount: u128) -> Option<u128> {
        self.ft(payer, amount, "", 100)
    }

    pub fn buy_msg(version: u64, hash: &str, pk: u8) -> String {
        json!({"kind": "buy", "listing_version": version, "known_keys_hash": hash,
               "buyer_runner_pk": key_str(pk)})
        .to_string()
    }

    /// Buy as the honest client does: current version and key hash.
    pub fn buy(&mut self, buyer: &str, amount: u128, pk: u8) -> Option<u128> {
        let v = self.c.m.listing_version;
        let h = self.c.known_keys_hash();
        self.ft(buyer, amount, &Self::buy_msg(v, &h, pk), 300)
    }

    /// `who` pays `PAYER_ROW_DEPOSIT` NEAR for its own payer row (A49).
    pub fn register(&mut self, who: &str) {
        let mut c = self.call(who);
        c.deposit = crate::PAYER_ROW_DEPOSIT;
        self.env(&c);
        self.c.storage_deposit(None, None);
    }

    pub fn advance(&mut self, ns: u64) {
        self.now += ns;
    }

    pub fn m(&self) -> &Machine<crate::meter_store::NearStore> {
        &self.c.m
    }

    /// FAULT INJECTION for A24 (simulator only, not in the wasm): lower the
    /// treasury as if some path had drained it, to prove `buy` refuses.
    pub fn inject_treasury_drain(&mut self, amount: u128) {
        self.c.m.treasury -= amount;
        self.c.m.held -= amount;
    }
}

/// A real contract has its own storage; the mocked runtime has one per
/// thread. Each simulator keeps its own storage map and swaps it in when it
/// sets up a call, so simulators alive at once in one thread (the verify
/// tool) never see each other's storage collections (`used_sigs`).
type Storage = std::collections::HashMap<Vec<u8>, Vec<u8>>;

thread_local! {
    static STORES: std::cell::RefCell<std::collections::HashMap<u64, Storage>> =
        Default::default();
    /// Storage usage each simulator's collections reached (bytes), so a
    /// call that overwrites a row starts from the real usage, not 0.
    static USAGE: std::cell::RefCell<std::collections::HashMap<u64, u64>> =
        Default::default();
    static CURRENT: std::cell::Cell<u64> = const { std::cell::Cell::new(0) };
    /// The `storage` the current call was given on top of the tracked usage.
    static LAST_BASE: std::cell::Cell<u64> = const { std::cell::Cell::new(0) };
    static NEXT_ID: std::cell::Cell<u64> = const { std::cell::Cell::new(1) };
}

impl Drop for Sim {
    fn drop(&mut self) {
        STORES.with(|st| st.borrow_mut().remove(&self.id));
    }
}

/// `AgentExit` has no `Default`; build a throwaway value without touching the
/// mocked runtime (overwritten right after in `Sim::new`).
fn unsafe_placeholder() -> AgentExit {
    AgentExit {
        m: Machine::with_store("placeholder", "placeholder"),
        token: acc(TOKEN),
        admin: acc(TOKEN),
        signers: Default::default(),
        used_sigs: crate::verified::UsedSigs::new(b'p'),
        payer_deposits: 0,
    }
}

pub fn logs() -> Vec<String> {
    get_logs()
}

/// Events with this name from the logs, as JSON `data[0]`.
pub fn events(name: &str) -> Vec<Value> {
    get_logs()
        .iter()
        .filter_map(|l| l.strip_prefix("EVENT_JSON:"))
        .filter_map(|j| serde_json::from_str::<Value>(j).ok())
        .filter(|v| v["event"] == name)
        .map(|v| v["data"][0].clone())
        .collect()
}

/// Key actions the contract scheduled on its own account, as
/// ("delete" | "add_fc" | "add_full", key) pairs.
pub fn key_actions(agent: &str) -> Vec<(String, String)> {
    use near_sdk::mock::MockAction;
    let mut out = vec![];
    for r in get_created_receipts() {
        if r.receiver_id.as_str() != agent {
            continue;
        }
        for a in r.actions {
            match a {
                MockAction::DeleteKey { public_key, .. } => {
                    out.push(("delete".into(), public_key.to_string()))
                }
                MockAction::AddKeyWithFunctionCall {
                    public_key,
                    receiver_id,
                    method_names,
                    ..
                } => {
                    assert_eq!(
                        receiver_id.as_str(),
                        agent,
                        "runner key must target the agent"
                    );
                    assert_eq!(method_names.join(","), RUNNER_METHODS);
                    out.push(("add_fc".into(), public_key.to_string()))
                }
                MockAction::AddKeyWithFullAccess { public_key, .. } => {
                    out.push(("add_full".into(), public_key.to_string()))
                }
                _ => {}
            }
        }
    }
    out
}

/// Function calls the contract scheduled on itself: (method, prepaid gas).
pub fn self_calls(agent: &str) -> Vec<(String, Gas)> {
    use near_sdk::mock::MockAction;
    let mut out = vec![];
    for r in get_created_receipts() {
        if r.receiver_id.as_str() != agent {
            continue;
        }
        for a in r.actions {
            if let MockAction::FunctionCallWeight {
                method_name,
                prepaid_gas,
                ..
            } = a
            {
                out.push((String::from_utf8_lossy(&method_name).into(), prepaid_gas));
            }
        }
    }
    out
}

/// `sign` calls the contract scheduled on the MPC signer: (args, deposit, gas).
pub fn mpc_sign_calls() -> Vec<(Value, NearToken, Gas)> {
    use near_sdk::mock::MockAction;
    let mut out = vec![];
    for r in get_created_receipts() {
        if r.receiver_id.as_str() != MPC_SIGNER {
            continue;
        }
        for a in r.actions {
            if let MockAction::FunctionCallWeight {
                method_name,
                args,
                attached_deposit,
                prepaid_gas,
                ..
            } = a
            {
                if method_name == b"sign" {
                    out.push((
                        serde_json::from_slice(&args).expect("json args"),
                        attached_deposit,
                        prepaid_gas,
                    ));
                }
            }
        }
    }
    out
}

/// Receipts the contract scheduled on itself that carry a key action.
pub fn key_receipts(agent: &str) -> usize {
    use near_sdk::mock::MockAction;
    get_created_receipts()
        .iter()
        .filter(|r| r.receiver_id.as_str() == agent)
        .filter(|r| {
            r.actions.iter().any(|a| {
                matches!(
                    a,
                    MockAction::DeleteKey { .. }
                        | MockAction::AddKeyWithFunctionCall { .. }
                        | MockAction::AddKeyWithFullAccess { .. }
                )
            })
        })
        .count()
}

/// `ft_transfer` calls the contract scheduled on the token: (receiver, amount).
pub fn ft_transfers() -> Vec<(String, u128)> {
    use near_sdk::mock::MockAction;
    let mut out = vec![];
    for r in get_created_receipts() {
        if r.receiver_id.as_str() != TOKEN {
            continue;
        }
        for a in r.actions {
            if let MockAction::FunctionCallWeight {
                method_name, args, ..
            } = a
            {
                if method_name == b"ft_transfer" {
                    let v: Value = serde_json::from_slice(&args).expect("json args");
                    out.push((
                        v["receiver_id"].as_str().unwrap_or_default().to_string(),
                        v["amount"]
                            .as_str()
                            .unwrap_or("0")
                            .parse()
                            .unwrap_or_default(),
                    ));
                }
            }
        }
    }
    out
}

// ------------------------------------------------------------- fixed world

/// `translator` before listing (SCENARIOS.md "Fixed world"): 6 payers x 5
/// payments x 12.00 on days 0-4; 2 self-payments by alice x 50.00 and one
/// payment by x.alice of 40.00 on day 5. Runner key set by alice; the clock
/// ends on day 10.
pub fn translator_world() -> Sim {
    let mut s = Sim::new(AGENT, ALICE);
    s.by_1y(ALICE).set_runner_key(key(ALICE_RUNNER)).detach();
    s.runner_key_cb(true, vec![], vec![], key_str(ALICE_RUNNER));
    // 10 of the 30 payer payments carry a NEAR AI-shaped signature (TEST
    // FIXTURES): all 6 payers on day 0, p1-p4 on day 1. 10 x 12.00 = 120.00.
    // The six customers registered payer rows (A49).
    for p in PAYERS {
        s.register(p);
    }
    let mut signed = 0;
    for day in 0..5u64 {
        s.now = T0 + day * DAY + 3_600 * agent_core::NANOS_PER_SEC;
        for p in PAYERS {
            let r = if signed < 10 {
                signed += 1;
                s.ft(p, 12 * USD, &job_msg(&job(signed - 1)), 100)
            } else {
                s.pay(p, 12 * USD)
            };
            assert_eq!(r, Some(0));
        }
    }
    s.now = T0 + 5 * DAY;
    assert_eq!(s.pay(ALICE, 50 * USD), Some(0));
    assert_eq!(s.pay(ALICE, 50 * USD), Some(0));
    assert_eq!(s.pay(X_ALICE, 40 * USD), Some(0));
    s.now = T0 + 10 * DAY;
    s
}

/// `cheat`: 500.00 gross, all from alice and alice's sub-accounts.
pub fn cheat_world() -> Sim {
    let mut s = Sim::new(CHEAT, ALICE);
    s.by_1y(ALICE).set_runner_key(key(ALICE_RUNNER)).detach();
    s.runner_key_cb(true, vec![], vec![], key_str(ALICE_RUNNER));
    for day in 0..5u64 {
        s.now = T0 + day * DAY;
        assert_eq!(s.pay(ALICE, 60 * USD), Some(0));
        assert_eq!(s.pay(X_ALICE, 40 * USD), Some(0));
    }
    s.now = T0 + 10 * DAY;
    s
}

/// `translator` listed at 1,000.00 with default terms (30% earn-out).
pub fn listed_translator() -> Sim {
    let mut s = translator_world();
    s.by_1y(ALICE)
        .list_for_sale(U128(1_000 * USD), CODE_HASH.into(), None);
    s
}

/// `translator` sold to bob; the seller payout landed.
pub fn sold_translator() -> Sim {
    let mut s = listed_translator();
    assert_eq!(s.buy(BOB, 1_000 * USD, BOB_RUNNER), None);
    let sale_id = s.m().pending.as_ref().expect("settling").sale_id;
    assert_eq!(s.callback(true).on_handover(sale_id).0, 0);
    s.callback(true)
        .on_transfer_out(OutKind::SellerPayout, acc(ALICE), U128(700 * USD), None);
    s
}

/// The demo limits (feature 12): price floor 5.00, daily cap 50.00, payee
/// `summarizer`, split 80% owner / 20% treasury.
pub fn demo_limits() -> LimitsArgs {
    LimitsArgs {
        price_floor: U128(5 * USD),
        daily_cap: U128(50 * USD),
        allowed_payees: vec![acc(SUMMARIZER)],
        split: Split {
            owner_bps: 8_000,
            treasury_bps: 2_000,
        },
        base: None,
    }
}

/// `translator` after the sale: bob set the demo limits, then two customers
/// paid 25.00 for a job each (treasury 10.00). Demo beat 1 of 3.
pub fn business_world() -> Sim {
    let mut s = sold_translator();
    s.by_1y(BOB).set_limits(demo_limits());
    s.now += 3_600 * agent_core::NANOS_PER_SEC;
    assert_eq!(
        s.ft(
            PAYERS[0],
            25 * USD,
            r#"{"kind":"pay","job_id":"job-b1"}"#,
            100
        ),
        Some(0)
    );
    assert_eq!(
        s.ft(
            PAYERS[1],
            25 * USD,
            r#"{"kind":"pay","job_id":"job-b2"}"#,
            100
        ),
        Some(0)
    );
    s
}

// ------------------------------------------------------- signature fixtures

/// Address of the LOCAL TEST signer (secp256k1 key 0x11..11) used by the
/// fixtures in `fixtures/test-signatures/`. Not a NEAR AI Cloud key.
pub fn test_signer() -> String {
    fixtures()["test_signer"]
        .as_str()
        .expect("test_signer")
        .to_string()
}

/// `fixtures/test-signatures/translator.json` (TEST FIXTURES, local keys).
pub fn fixtures() -> Value {
    serde_json::from_str(include_str!(
        "../../../fixtures/test-signatures/translator.json"
    ))
    .expect("fixture json")
}

/// The `nearai` proof object of a fixture entry.
pub fn proof(entry: &Value) -> Value {
    json!({
        "model": entry["model"], "request": entry["request"],
        "response_sha256": entry["response_sha256"], "signature": entry["signature"],
    })
}

/// `pay` msg for a job, with the fixture entry's signature attached.
pub fn job_msg(entry: &Value) -> String {
    json!({"kind": "pay", "job_id": entry["job_id"], "nearai": proof(entry)}).to_string()
}

/// Fixture entry `i` (0-based) of the 10 translator jobs.
pub fn job(i: usize) -> Value {
    fixtures()["entries"][i].clone()
}
