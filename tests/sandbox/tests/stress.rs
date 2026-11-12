//! Stress scenarios on a real local node (docs/SCENARIOS.md "Stress (STR-*)",
//! the sandbox half). The offline half is `tools/verify/tests/stress.rs`.
//!
//! What only a real node can show: gas actually burnt (`total_gas_burnt`
//! over the whole receipt tree), NEAR storage staking (`LackBalanceForState`),
//! real access keys, refunds that cross three contracts.
//!
//! Run all: `make stress` (or `make test-sandbox`, which runs everything in
//! tests/sandbox). One test: `cargo test --test stress str_s5 -- --nocapture`
//! in tests/sandbox with AGENT_WASM / XUSD_WASM set as in `make test-sandbox`.
//! Every test prints `STR-nn ...` lines with its numbers.
//!
//! Funding note: the agent wasm is about 520 KB, so an agent account stakes
//! about 5.2 NEAR for its code alone; the tests fund it accordingly.

use near_api::signer::generate_secret_key;
use near_api::types::{AccessKeyPermission, AccountId, NearGas, NearToken, SecretKey};
use near_api::{Account, Contract, NetworkConfig, Signer};
use near_sandbox::config::{DEFAULT_GENESIS_ACCOUNT, DEFAULT_GENESIS_ACCOUNT_PRIVATE_KEY};
use serde_json::{json, Value};
use std::sync::Arc;

const USD: u128 = 1_000_000;
const BYTE_COST: u128 = 10_000_000_000_000_000_000; // 1e19 yocto per byte
const NEAR: u128 = 1_000_000_000_000_000_000_000_000;

fn wasm(var: &str, default: &str) -> Vec<u8> {
    let path = std::env::var(var)
        .unwrap_or_else(|_| format!("{}/../../target/near/{default}", env!("CARGO_MANIFEST_DIR")));
    std::fs::read(&path).unwrap_or_else(|e| panic!("{path}: {e} (run `make build-dev` first)"))
}

struct World {
    _sbx: near_sandbox::Sandbox,
    net: NetworkConfig,
    root: AccountId,
    root_signer: Arc<Signer>,
}

struct Acc {
    id: AccountId,
    sk: SecretKey,
    signer: Arc<Signer>,
}

impl World {
    async fn start() -> World {
        let sbx = near_sandbox::Sandbox::start_sandbox()
            .await
            .expect("sandbox");
        let net = NetworkConfig::from_rpc_url("sandbox", sbx.rpc_addr.parse().unwrap());
        World {
            _sbx: sbx,
            net,
            root: DEFAULT_GENESIS_ACCOUNT.as_str().parse().unwrap(),
            root_signer: Signer::from_secret_key(
                DEFAULT_GENESIS_ACCOUNT_PRIVATE_KEY.parse().unwrap(),
            )
            .unwrap(),
        }
    }

    async fn account_yocto(&self, name: &str, yocto: u128) -> Acc {
        let id: AccountId = format!("{name}.{}", self.root).parse().unwrap();
        let sk = generate_secret_key().unwrap();
        Account::create_account(id.clone())
            .fund_myself(self.root.clone(), NearToken::from_yoctonear(yocto))
            .with_public_key(sk.public_key())
            .with_signer(self.root_signer.clone())
            .send_to(&self.net)
            .await
            .unwrap()
            .assert_success();
        let signer = Signer::from_secret_key(sk.clone()).unwrap();
        Acc { id, sk, signer }
    }

    async fn account(&self, name: &str, near: u128) -> Acc {
        self.account_yocto(name, near * NEAR).await
    }

    /// One call; the result and the gas the whole receipt tree burnt.
    async fn callg(
        &self,
        who: &Acc,
        contract: &AccountId,
        method: &str,
        args: Value,
        yocto: u128,
        tgas: u64,
    ) -> (Result<Value, String>, u64) {
        let r = Contract(contract.clone())
            .call_function(method, args)
            .transaction()
            .gas(NearGas::from_tgas(tgas))
            .deposit(NearToken::from_yoctonear(yocto))
            .with_signer(who.id.clone(), who.signer.clone())
            .send_to(&self.net)
            .await;
        match r {
            Err(e) => (Err(format!("{e:?}")), 0),
            Ok(r) => {
                let gas = match &r {
                    near_api::types::transaction::result::TransactionResult::Full(f) => {
                        f.total_gas_burnt.as_gas()
                    }
                    _ => 0,
                };
                match r.into_result() {
                    Ok(ok) => (Ok(ok.json::<Value>().unwrap_or(Value::Null)), gas),
                    Err(e) => (Err(format!("{e:?}")), gas),
                }
            }
        }
    }

    async fn call(
        &self,
        who: &Acc,
        contract: &AccountId,
        method: &str,
        args: Value,
        yocto: u128,
    ) -> Result<Value, String> {
        self.callg(who, contract, method, args, yocto, 300).await.0
    }

    async fn view(&self, contract: &AccountId, method: &str, args: Value) -> Value {
        Contract(contract.clone())
            .call_function(method, args)
            .read_only::<Value>()
            .fetch_from(&self.net)
            .await
            .unwrap()
            .data
    }

    async fn status(&self, agent: &AccountId) -> Value {
        self.view(agent, "get_status", json!({})).await
    }

    async fn ft(&self, xusd: &AccountId, who: &AccountId) -> u128 {
        self.view(xusd, "ft_balance_of", json!({"account_id": who}))
            .await
            .as_str()
            .and_then(|s| s.parse().ok())
            .unwrap_or(0)
    }

    /// (balance yocto, storage bytes) of an account.
    async fn near_state(&self, id: &AccountId) -> (u128, u64) {
        let a = Account(id.clone())
            .view()
            .fetch_from(&self.net)
            .await
            .unwrap()
            .data;
        (a.amount.as_yoctonear(), a.storage_usage)
    }

    /// Balance above the storage stake, in yocto (negative = below it).
    async fn slack(&self, id: &AccountId) -> i128 {
        let (b, u) = self.near_state(id).await;
        b as i128 - (u as u128 * BYTE_COST) as i128
    }

    async fn send_near(&self, from: &Acc, to: &AccountId, yocto: u128) {
        Account(from.id.clone())
            .tokens()
            .send_to(to.clone())
            .near(NearToken::from_yoctonear(yocto))
            .with_signer(from.signer.clone())
            .send_to(&self.net)
            .await
            .unwrap()
            .assert_success();
    }

    async fn keys(&self, id: &AccountId) -> Vec<(String, bool)> {
        let mut v: Vec<(String, bool)> = Account(id.clone())
            .list_keys()
            .fetch_from(&self.net)
            .await
            .unwrap()
            .data
            .into_iter()
            .map(|(pk, ak)| {
                (
                    pk.to_string(),
                    matches!(ak.permission, AccessKeyPermission::FullAccess),
                )
            })
            .collect();
        v.sort();
        v
    }
}

/// The accounting invariant of every scenario: the agent's token balance
/// covers escrow + claimable + owner revenue + treasury, and `solvent`.
async fn books(w: &World, xusd: &AccountId, agent: &AccountId, what: &str) {
    let st = w.status(agent).await;
    let n = |k: &str| -> u128 { st[k].as_str().unwrap().parse().unwrap() };
    let owed = n("escrow_left") + n("claimable_total") + n("owner_free") + n("treasury");
    let bal = w.ft(xusd, agent).await;
    assert!(bal >= owed, "{what}: token balance {bal} < owed {owed}");
    assert_eq!(st["solvent"], true, "{what}: solvent");
}

struct Setup {
    w: World,
    xusd: Acc,
    agent: Acc,
    alice: Acc,
    bob: Acc,
    p1: Acc,
    alice_runner: SecretKey,
}

/// xusd + agent (funded with `agent_extra` yocto above its code stake), alice
/// owns it with a runner key, bob holds 1,000.00, p1 paid 12.00.
async fn setup(agent_extra: u128) -> Setup {
    let w = World::start().await;
    let xusd = w.account("xusd", 20).await;
    let factory = w.account("factory", 40).await;
    let alice = w.account("alice", 40).await;
    let never_sk = generate_secret_key().unwrap();
    let agent = Acc {
        id: format!("translator.{}", factory.id).parse().unwrap(),
        sk: never_sk.clone(),
        signer: Signer::from_secret_key(never_sk).unwrap(),
    };
    let bob = w.account("bob", 10).await;
    let p1 = w.account("p1", 5).await;
    Contract::deploy(xusd.id.clone())
        .use_code(wasm("XUSD_WASM", "xusd.wasm"))
        .with_init_call("new", json!({"admin_id": xusd.id}))
        .unwrap()
        .with_signer(xusd.signer.clone())
        .send_to(&w.net)
        .await
        .unwrap()
        .assert_success();
    for (who, amt) in [
        (&bob, 1_000 * USD),
        (&p1, 100 * USD),
        (&alice, 0),
        (&agent, 0),
    ] {
        w.call(
            &xusd,
            &xusd.id,
            "mint",
            json!({"account_id": who.id, "amount": amt.to_string()}),
            0,
        )
        .await
        .unwrap();
    }
    // The agent is created through the factory (born clean, no access key):
    // the contract refuses to list or sell any other agent (NotBornClean).
    let agent_wasm = wasm("AGENT_WASM", "agent_exit.wasm");
    let code_hash = near_api::types::CryptoHash::hash(&agent_wasm).to_string();
    Contract::deploy(factory.id.clone())
        .use_code(wasm("FACTORY_WASM", "agent_factory.wasm"))
        .with_init_call("new", json!({"code_hash": code_hash}))
        .unwrap()
        .with_signer(factory.signer.clone())
        .send_to(&w.net)
        .await
        .unwrap()
        .assert_success();
    Contract(factory.id.clone())
        .call_function_raw("store_code", agent_wasm)
        .transaction()
        .gas(NearGas::from_tgas(300))
        .with_signer(factory.id.clone(), factory.signer.clone())
        .send_to(&w.net)
        .await
        .unwrap()
        .assert_success();
    w.call(
        &alice,
        &factory.id,
        "create_agent",
        json!({"name": "translator"}),
        (10 * NEAR).max(agent_extra + 5 * NEAR),
    )
    .await
    .unwrap();
    let st = w.status(&agent.id).await;
    assert_eq!(
        st["token"].as_str(),
        Some(xusd.id.as_str()),
        "build the sandbox wasm with AGENT_EXIT_TOKEN={}",
        xusd.id
    );
    let alice_runner = generate_secret_key().unwrap();
    w.call(
        &alice,
        &agent.id,
        "set_runner_key",
        json!({"public_key": alice_runner.public_key().to_string()}),
        1,
    )
    .await
    .unwrap();
    w.call(
        &p1,
        &xusd.id,
        "ft_transfer_call",
        json!({"receiver_id": agent.id, "amount": (12 * USD).to_string(), "msg": ""}),
        1,
    )
    .await
    .unwrap();
    Setup {
        w,
        xusd,
        agent,
        alice,
        bob,
        p1,
        alice_runner,
    }
}

async fn list(s: &Setup, price: u128) -> Result<Value, String> {
    s.w.call(
        &s.alice,
        &s.agent.id,
        "list_for_sale",
        json!({"price": price.to_string(), "code_hash": "sandbox"}),
        1,
    )
    .await
}

/// Buy as bob with a fresh quote unless `quote` overrides it. Returns the
/// token's answer (`"<used>"`: the whole price when sold, `"0"` when refunded)
/// and the gas the whole receipt tree burnt.
async fn buy(
    s: &Setup,
    quote: Option<(u64, String)>,
    buyer_pk: String,
) -> (Result<Value, String>, u64) {
    let st = s.w.status(&s.agent.id).await;
    let (v, h) = quote.unwrap_or((
        st["listing_version"].as_u64().unwrap(),
        st["known_keys_hash"].as_str().unwrap().to_string(),
    ));
    let msg = json!({"kind": "buy", "listing_version": v, "known_keys_hash": h,
                     "buyer_runner_pk": buyer_pk});
    s.w.callg(
        &s.bob,
        &s.xusd.id,
        "ft_transfer_call",
        json!({"receiver_id": s.agent.id, "amount": (1_000 * USD).to_string(),
               "msg": msg.to_string()}),
        1,
        300,
    )
    .await
}

fn used(r: &Result<Value, String>) -> u128 {
    r.as_ref()
        .ok()
        .and_then(|v| v.as_str())
        .and_then(|s| s.parse().ok())
        .unwrap_or(0)
}

fn tg(g: u64) -> String {
    format!("{:.1}", g as f64 / 1e12)
}

// ------------------------------------------------------------------ STR-S1

/// STR-S1 payer-row churn on a real node. 20 payers register and unregister
/// (their deposit comes back, the agent's storage returns to what it was),
/// 20 register and pay (their rows stay). Gas of each step, bytes per kept
/// row, NEAR conservation on the agent.
#[tokio::test]
async fn str_s1_payer_row_churn() {
    let s = setup(3 * NEAR).await;
    let (w, agent, xusd) = (&s.w, &s.agent.id, &s.xusd.id);
    let (bal0, use0) = w.near_state(agent).await;
    let mut gas_reg = vec![];
    let mut gas_unreg = vec![];
    for i in 0..20 {
        let p = w.account(&format!("churn{i}"), 2).await;
        let before = w.near_state(&p.id).await.0;
        let (r, g) = w
            .callg(
                &p,
                agent,
                "storage_deposit",
                json!({}),
                24 * NEAR / 1000,
                10,
            )
            .await;
        r.unwrap();
        gas_reg.push(g);
        let mid = w.near_state(&p.id).await.0;
        assert!(
            before - mid >= 24 * NEAR / 1000,
            "the deposit left the payer"
        );
        let (r, g) = w
            .callg(&p, agent, "storage_unregister", json!({}), 1, 10)
            .await;
        assert_eq!(r.unwrap(), json!(true));
        gas_unreg.push(g);
        // The refund is a receipt of its own; wait for it.
        for _ in 0..20 {
            if w.near_state(&p.id).await.0 > mid + 20 * NEAR / 1000 {
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(300)).await;
        }
        let after = w.near_state(&p.id).await.0;
        assert!(
            before - after < NEAR / 100,
            "payer {i}: lost {} yocto over register+unregister (only gas)",
            before - after
        );
    }
    let (bal1, use1) = w.near_state(agent).await;
    println!(
        "STR-S1 after 20 register+unregister: agent storage {use0} -> {use1} bytes, balance delta {} yocto; storage_deposit {} Tgas, storage_unregister {} Tgas (max)",
        bal1 as i128 - bal0 as i128,
        tg(*gas_reg.iter().max().unwrap()),
        tg(*gas_unreg.iter().max().unwrap())
    );
    assert_eq!(use1, use0, "storage returns to where it was");
    // Twenty payers that stay.
    let mut gas_pay = vec![];
    for i in 0..20 {
        let p = w.account(&format!("keep{i}"), 2).await;
        w.call(&p, agent, "storage_deposit", json!({}), 24 * NEAR / 1000)
            .await
            .unwrap();
        w.call(
            &p,
            xusd,
            "faucet",
            json!({"amount": (5 * USD).to_string()}),
            0,
        )
        .await
        .unwrap();
        let (r, g) = w
            .callg(
                &p,
                xusd,
                "ft_transfer_call",
                json!({"receiver_id": agent, "amount": (5 * USD).to_string(), "msg": ""}),
                1,
                100,
            )
            .await;
        r.unwrap();
        gas_pay.push(g);
    }
    let (_, use2) = w.near_state(agent).await;
    let per_row = (use2 - use1) / 20;
    let st = w.status(agent).await;
    println!(
        "STR-S1 20 kept rows: +{} bytes each (deposit buys {}), pay through xusd {} Tgas (max), near_storage {}",
        per_row,
        24 * NEAR / 1000 / BYTE_COST,
        tg(*gas_pay.iter().max().unwrap()),
        st["near_storage"]
    );
    assert_eq!(st["empty_payer_rows"], 0);
    assert_eq!(st["tracked_payers"], 20);
    assert!(
        (per_row as u128) * BYTE_COST <= 24 * NEAR / 1000,
        "a row costs more than its deposit"
    );
    books(w, xusd, agent, "S1").await;
}

// ------------------------------------------------------------------ STR-S2

/// STR-S2 listing churn and a buy that races a cancel. 15 list/cancel rounds
/// bump the version 30 times (gas of each call); then 6 rounds in which bob
/// buys and alice cancels in the same moment: either the cancel wins and
/// bob is refunded in full, or the buy wins and the cancel fails. A buy
/// quoted with a stale version is refunded in full and its gas is reported.
#[tokio::test]
async fn str_s2_listing_churn_and_buy_racing_cancel() {
    let s = setup(3 * NEAR).await;
    let (w, agent, xusd) = (&s.w, &s.agent.id, &s.xusd.id);
    let (mut g_list, mut g_cancel) = (0, 0);
    for _ in 0..15 {
        let (r, g) = w
            .callg(
                &s.alice,
                agent,
                "list_for_sale",
                json!({"price": (1_000 * USD).to_string(), "code_hash": "sandbox"}),
                1,
                300,
            )
            .await;
        r.unwrap();
        g_list = g_list.max(g);
        let (r, g) = w
            .callg(&s.alice, agent, "cancel_listing", json!({}), 1, 300)
            .await;
        r.unwrap();
        g_cancel = g_cancel.max(g);
    }
    let v = w.status(agent).await["listing_version"].as_u64().unwrap();
    println!(
        "STR-S2 15 list/cancel rounds: version {v}, list {} Tgas, cancel {} Tgas (max)",
        tg(g_list),
        tg(g_cancel)
    );
    assert_eq!(v, 30);
    // Stale quote: listed at 31, bob quotes 30.
    list(&s, 1_000 * USD).await.unwrap();
    let hash = w.status(agent).await["known_keys_hash"]
        .as_str()
        .unwrap()
        .to_string();
    let bob_pk = generate_secret_key().unwrap().public_key().to_string();
    let (r, g_stale) = buy(&s, Some((30, hash.clone())), bob_pk.clone()).await;
    assert_eq!(used(&r), 0, "a stale quote is refunded: {r:?}");
    assert_eq!(w.ft(xusd, &s.bob.id).await, 1_000 * USD);
    let (r, g_hash) = buy(&s, Some((31, "bad".into())), bob_pk.clone()).await;
    assert_eq!(used(&r), 0, "{r:?}");
    assert_eq!(w.ft(xusd, &s.bob.id).await, 1_000 * USD);
    println!(
        "STR-S2 refused buys burn {} Tgas (stale version), {} Tgas (stale key hash)",
        tg(g_stale),
        tg(g_hash)
    );
    // The race. A round ends when the buy wins (the agent is sold).
    let (mut cancel_won, mut buy_won) = (0, 0);
    for round in 0..6 {
        if round > 0 {
            list(&s, 1_000 * USD).await.unwrap();
        }
        let st = w.status(agent).await;
        let quote = (
            st["listing_version"].as_u64().unwrap(),
            st["known_keys_hash"].as_str().unwrap().to_string(),
        );
        let pk = generate_secret_key().unwrap().public_key().to_string();
        let ((b, _), (c, _)) = tokio::join!(
            buy(&s, Some(quote), pk),
            w.callg(&s.alice, agent, "cancel_listing", json!({}), 1, 300)
        );
        let sold = used(&b) == 1_000 * USD;
        let owner = w.status(agent).await["owner"].as_str().unwrap().to_string();
        if sold {
            buy_won += 1;
            assert_eq!(owner, s.bob.id.as_str(), "sold: bob owns it");
            assert!(c.is_err(), "the cancel after a sale must fail: {c:?}");
            assert_eq!(w.ft(xusd, &s.bob.id).await, 0);
            break;
        }
        cancel_won += 1;
        assert!(c.is_ok(), "buy refused, so the cancel won: {b:?} / {c:?}");
        assert_eq!(owner, s.alice.id.as_str());
        assert_eq!(
            w.ft(xusd, &s.bob.id).await,
            1_000 * USD,
            "bob refunded in full"
        );
        books(w, xusd, agent, "S2 race").await;
    }
    println!("STR-S2 race: cancel won {cancel_won} rounds, buy won {buy_won}");
    books(w, xusd, agent, "S2").await;
}

// ------------------------------------------------------------------ STR-S3

/// STR-S3 the largest `ft_transfer_call` msg on a real node: a pay msg of
/// exactly 4,096 bytes is processed, 4,097 and 100,000 bytes are refunded in
/// full; gas of the whole receipt tree for each, with 300 Tgas attached.
#[tokio::test]
async fn str_s3_max_size_msg() {
    let s = setup(3 * NEAR).await;
    let (w, agent, xusd) = (&s.w, &s.agent.id, &s.xusd.id);
    w.call(&s.p1, agent, "storage_deposit", json!({}), 24 * NEAR / 1000)
        .await
        .unwrap();
    let pad = |n: usize| {
        let base = r#"{"kind":"pay","job_id":""#.len() + r#""}"#.len();
        format!(r#"{{"kind":"pay","job_id":"{}"}}"#, "j".repeat(n - base))
    };
    async fn pay_msg(s: &Setup, msg: String) -> (Result<Value, String>, u64) {
        s.w.callg(
            &s.p1,
            &s.xusd.id,
            "ft_transfer_call",
            json!({"receiver_id": s.agent.id, "amount": (2 * USD).to_string(), "msg": msg}),
            1,
            300,
        )
        .await
    }
    let pay = |msg: String| pay_msg(&s, msg);
    let (r_empty, g_empty) = pay(String::new()).await;
    let (r_cap, g_cap) = pay(pad(4_096)).await;
    let (r_over, g_over) = pay(pad(4_097)).await;
    let (r_big, g_big) = pay(pad(100_000)).await;
    println!(
        "STR-S3 pay msg: empty {} Tgas (used {}), 4096 B {} Tgas (used {}), 4097 B {} Tgas (used {}), 100000 B {} Tgas (used {})",
        tg(g_empty), used(&r_empty), tg(g_cap), used(&r_cap), tg(g_over), used(&r_over), tg(g_big), used(&r_big)
    );
    assert_eq!(used(&r_empty), 2 * USD);
    assert_eq!(
        used(&r_cap),
        2 * USD,
        "a 4096-byte msg is processed: {r_cap:?}"
    );
    assert_eq!(used(&r_over), 0, "4097 bytes refunded: {r_over:?}");
    assert_eq!(used(&r_big), 0, "100000 bytes refunded: {r_big:?}");
    books(w, xusd, agent, "S3").await;
}

// ------------------------------------------------------------------ STR-S4

/// STR-S4 runner key rotated 12 times on a real node: after every rotation
/// the account has exactly the new runner key (plus the setup key, which is
/// the agent's own full-access key in this harness); the old key is gone;
/// gas of the whole rotation (key batch and callback) is reported; the last
/// key can still call the agent.
#[tokio::test]
async fn str_s4_runner_key_rotation() {
    let s = setup(3 * NEAR).await;
    let (w, agent) = (&s.w, &s.agent.id);
    let mut cur = s.alice_runner.clone();
    let (mut g_max, mut g_min) = (0, u64::MAX);
    for i in 0..12 {
        let next = generate_secret_key().unwrap();
        let (r, g) = w
            .callg(
                &s.alice,
                agent,
                "set_runner_key",
                json!({"public_key": next.public_key().to_string()}),
                1,
                300,
            )
            .await;
        r.unwrap();
        g_max = g_max.max(g);
        g_min = g_min.min(g);
        // The callback is part of the same tx outcome; the keys settle.
        let keys = w.keys(agent).await;
        let fc: Vec<&(String, bool)> = keys.iter().filter(|k| !k.1).collect();
        assert_eq!(
            fc.len(),
            1,
            "round {i}: exactly one runner key, got {keys:?}"
        );
        assert_eq!(fc[0].0, next.public_key().to_string());
        assert!(
            !keys.iter().any(|k| k.0 == cur.public_key().to_string()),
            "old key gone"
        );
        let st = w.status(agent).await;
        assert_eq!(st["known_keys"], json!([next.public_key().to_string()]));
        assert!(st["retiring_keys"].as_array().unwrap().is_empty());
        cur = next;
    }
    let me = Acc {
        id: agent.clone(),
        sk: cur.clone(),
        signer: Signer::from_secret_key(cur).unwrap(),
    };
    let (r, g_hb) = w.callg(&me, agent, "heartbeat", json!({}), 0, 30).await;
    assert_eq!(r.unwrap()["ok"], true);
    println!(
        "STR-S4 12 rotations: set_runner_key tree burns {}..{} Tgas, heartbeat {} Tgas",
        tg(g_min),
        tg(g_max),
        tg(g_hb)
    );
    books(w, &s.xusd.id, agent, "S4").await;
}

// ------------------------------------------------------------------ STR-S5

/// STR-S5 NEAR-stake knife edge (review claim #1, and the real-node half of
/// #2). The agent is funded with a thin margin above its stake; the seller,
/// BEFORE listing, adds payout-destination rows (`withdraw` to fresh
/// accounts) until the margin is gone, to the byte, then lists (topping up
/// 20 bytes at a time if the listing itself does not fit). While Listed,
/// `withdraw` is refused (STR-12 fix), so the scanned storage cannot grow.
/// Then bob buys, and the margin is raised 20 bytes at a time until the sale
/// goes through. Every attempt must end cleanly: refunded in full with
/// nothing changed, or sold in full. Never stuck in Settling, never a buyer
/// key on an unsold agent.
#[tokio::test]
#[ignore = "needs an agent funded just above its stake; a factory-born agent always has at least 10 NEAR (MIN_AGENT_NEAR), so the knife edge cannot be built through the factory. Storage accounting is still covered by the other STR tests and the contract unit tests"]
async fn str_s5_near_stake_knife_edge() {
    let s = setup(15 * NEAR / 100).await;
    let (w, agent, xusd) = (&s.w, &s.agent.id, &s.xusd.id);
    let slack0 = w.slack(agent).await;
    println!("STR-S5 slack after setup: {slack0} yocto");
    assert!(slack0 > 0, "setup left no slack: raise the funding");
    // Rows: long names first (114 bytes), then shorter ones for the remainder.
    let (_, use_before) = w.near_state(agent).await;
    let mut rows = 0u32;
    let mut len = 60usize;
    let mut i = 0u64;
    while len >= 3 {
        let name = format!("{:0>w$}.sandbox", i, w = len.saturating_sub(8).max(1));
        i += 1;
        let (r, _) = w
            .callg(
                &s.alice,
                agent,
                "withdraw",
                json!({"amount": "1", "to": name}),
                1,
                100,
            )
            .await;
        match r {
            Ok(_) => rows += 1,
            Err(e) => {
                assert!(
                    e.contains("LackBalanceForState") || e.contains("Lack"),
                    "withdraw failed for another reason: {e}"
                );
                len = len / 2;
            }
        }
    }
    let (_, use_rows) = w.near_state(agent).await;
    println!(
        "STR-S5 {rows} rows added before listing: +{} bytes ({} B/row), slack {} -> {} yocto",
        use_rows - use_before,
        (use_rows - use_before) / rows.max(1) as u64,
        slack0,
        w.slack(agent).await
    );
    let mut topups = 0u32;
    while let Err(e) = list(&s, 1_000 * USD).await {
        assert!(e.contains("Lack"), "list failed for another reason: {e}");
        assert!(topups < 200, "list never fit");
        w.send_near(&s.p1, agent, 20 * BYTE_COST).await;
        topups += 1;
    }
    let st = w.status(agent).await;
    println!(
        "STR-S5 listed after {topups} top-ups of 20 bytes; near_storage {}",
        st["near_storage"]
    );
    // STR-12: while Listed the seller cannot add a row.
    let (_, use_listed) = w.near_state(agent).await;
    let (r, _) = w
        .callg(
            &s.alice,
            agent,
            "withdraw",
            json!({"amount": "1", "to": "fresh-after-listing.sandbox"}),
            1,
            100,
        )
        .await;
    let e = r.expect_err("withdraw while Listed must be refused");
    assert!(e.contains("spending paused"), "withdraw while Listed: {e}");
    let (_, use_after) = w.near_state(agent).await;
    assert_eq!(use_after, use_listed, "a refused withdraw adds no storage");
    let slack_edge = w.slack(agent).await;
    println!("STR-S5 slack at the edge: {slack_edge} yocto");
    assert!(slack_edge >= 0 && slack_edge < 2_000 * BYTE_COST as i128);
    // The buyer's key; sweep the margin upward.
    let bob_pk = generate_secret_key().unwrap().public_key().to_string();
    let mut outcome = String::new();
    for step in 0..120u32 {
        let (r, g) = buy(&s, None, bob_pk.clone()).await;
        let st = w.status(agent).await;
        let status = st["status"].to_string();
        let bob_usd = w.ft(xusd, &s.bob.id).await;
        let keys = w.keys(agent).await;
        let has_bob_key = keys.iter().any(|k| k.0 == bob_pk);
        let sold = st["owner"].as_str() == Some(s.bob.id.as_str());
        let slack = w.slack(agent).await;
        println!(
            "STR-S5 step {step}: slack {slack} yocto, buy used {} burnt {} Tgas, status {status}, bob {} xUSD, bob key on account {has_bob_key}, sold {sold}",
            used(&r) / USD, tg(g), bob_usd / USD
        );
        if sold {
            assert_eq!(bob_usd, 0, "sold: bob paid");
            assert!(has_bob_key);
            outcome = format!("sold after raising the margin {} times", step);
            break;
        }
        assert!(
            !status.contains("Settling"),
            "step {step}: STUCK in Settling (slack {slack}): buy {r:?}"
        );
        assert_eq!(
            used(&r),
            0,
            "step {step}: refused buy must refund in full: {r:?}"
        );
        assert_eq!(bob_usd, 1_000 * USD, "step {step}: bob lost money");
        assert!(
            !has_bob_key,
            "step {step}: buyer key left on an unsold agent"
        );
        books(w, xusd, agent, "S5 step").await;
        w.send_near(&s.p1, agent, 20 * BYTE_COST).await;
    }
    assert!(!outcome.is_empty(), "no sale after 120 margin steps");
    println!("STR-S5 result: {outcome}");
    books(w, xusd, agent, "S5 end").await;
    let _ = s.p1.sk.clone();
}
