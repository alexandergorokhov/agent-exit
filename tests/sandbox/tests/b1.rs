//! near-sandbox tests for batch B1 (SCENARIOS.md layer `sbx`): the parts the
//! mocked runtime cannot show, on a real local node.
//!
//! - S5: one buy moves the deed, the agent's on-chain key list goes from
//!   [alice runner] to [bob runner], alice receives 700.00, 300.00 stays.
//! - A17 / A21: the old runner key and the deleted setup key now fail with
//!   the node's real error.
//! - E5: a key batch that fails (buyer key already on the account) rolls back
//!   and the token refunds bob in full.
//!
//! Run with `make test-sandbox` (builds the wasm first). Needs the
//! near-sandbox binary (downloaded on first run) and no other network.
//! WASM paths: AGENT_WASM / XUSD_WASM, default target/near/*.wasm.

use near_api::signer::generate_secret_key;
use near_api::types::{AccessKeyPermission, AccountId, NearGas, NearToken, SecretKey};
use near_api::{Account, Contract, NetworkConfig, Signer};
use near_sandbox::config::{DEFAULT_GENESIS_ACCOUNT, DEFAULT_GENESIS_ACCOUNT_PRIVATE_KEY};
use serde_json::{json, Value};
use std::sync::Arc;

const USD: u128 = 1_000_000;

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

    async fn account(&self, name: &str, near: u128) -> Acc {
        let id: AccountId = format!("{name}.{}", self.root).parse().unwrap();
        let sk = generate_secret_key().unwrap();
        Account::create_account(id.clone())
            .fund_myself(self.root.clone(), NearToken::from_near(near))
            .with_public_key(sk.public_key())
            .with_signer(self.root_signer.clone())
            .send_to(&self.net)
            .await
            .unwrap()
            .assert_success();
        let signer = Signer::from_secret_key(sk.clone()).unwrap();
        Acc { id, sk, signer }
    }

    async fn call(
        &self,
        who: &Acc,
        contract: &AccountId,
        method: &str,
        args: Value,
        yocto: u128,
    ) -> Result<Value, String> {
        // Runner keys are function-call keys with a gas allowance that the
        // node checks against the attached gas at a pessimistic gas price:
        // attach what the method needs (AGENTS.md "Gas per runner call").
        let tgas = match method {
            "heartbeat" => 30,
            "pay" => 100,
            "pay_base" => 300,
            _ => 300,
        };
        let r = Contract(contract.clone())
            .call_function(method, args)
            .transaction()
            .gas(NearGas::from_tgas(tgas))
            .deposit(NearToken::from_yoctonear(yocto))
            .with_signer(who.id.clone(), who.signer.clone())
            .send_to(&self.net)
            .await
            .map_err(|e| format!("{e:?}"))?;
        match r.into_result() {
            Ok(ok) => Ok(ok.json::<Value>().unwrap_or(Value::Null)),
            Err(e) => Err(format!("{e:?}")),
        }
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

/// Deploy xusd + the agent, seed one payment, list at 1,000.00.
/// Returns (world, xusd, agent, alice, bob, alice_runner_sk, setup_sk).
async fn listed() -> (World, AccountId, Acc, Acc, Acc, SecretKey, SecretKey) {
    let w = World::start().await;
    let xusd = w.account("xusd", 20).await;
    let agent = w.account("translator", 30).await;
    let alice = w.account("alice", 10).await;
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
    for (who, amt) in [(&bob, 1_000 * USD), (&p1, 100 * USD), (&alice, 0)] {
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
    w.call(
        &xusd,
        &xusd.id,
        "mint",
        json!({"account_id": agent.id, "amount": "0"}),
        0,
    )
    .await
    .unwrap();

    Contract::deploy(agent.id.clone())
        .use_code(wasm("AGENT_WASM", "agent_exit.wasm"))
        .with_init_call("new", json!({"owner_id": alice.id}))
        .unwrap()
        .with_signer(agent.signer.clone())
        .send_to(&w.net)
        .await
        .unwrap()
        .assert_success();
    // The token and admin are compiled in (A26): `make test-sandbox` builds
    // the wasm with AGENT_EXIT_TOKEN=xusd.sandbox.
    let st = w.view(&agent.id, "get_status", json!({})).await;
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
    // p1 pays 12.00 for a job.
    w.call(
        &p1,
        &xusd.id,
        "ft_transfer_call",
        json!({"receiver_id": agent.id, "amount": (12 * USD).to_string(), "msg": ""}),
        1,
    )
    .await
    .unwrap();
    w.call(
        &alice,
        &agent.id,
        "list_for_sale",
        json!({"price": (1_000 * USD).to_string(), "code_hash": "sandbox"}),
        1,
    )
    .await
    .unwrap();
    let setup = agent.sk.clone();
    (w, xusd.id, agent, alice, bob, alice_runner, setup)
}

async fn buy(
    w: &World,
    xusd: &AccountId,
    agent: &AccountId,
    bob: &Acc,
    buyer_pk: String,
) -> Result<Value, String> {
    let st = w.view(agent, "get_status", json!({})).await;
    let msg = json!({"kind": "buy", "listing_version": st["listing_version"],
                     "known_keys_hash": st["known_keys_hash"], "buyer_runner_pk": buyer_pk});
    w.call(
        bob,
        xusd,
        "ft_transfer_call",
        json!({"receiver_id": agent, "amount": (1_000 * USD).to_string(), "msg": msg.to_string()}),
        1,
    )
    .await
}

fn is_missing_key(err: &str) -> bool {
    [
        "AccessKeyNotFound",
        "does not exist",
        "UnknownAccessKey",
        "InvalidAccessKeyError",
    ]
    .iter()
    .any(|n| err.contains(n))
}

#[tokio::test]
async fn s5_a17_a21_buy_one_step_then_old_keys_fail() {
    let (w, xusd, agent, alice, bob, alice_runner, setup) = listed().await;

    // Locked account: delete the setup full-access key before the sale.
    Account(agent.id.clone())
        .delete_key(setup.public_key())
        .with_signer(agent.signer.clone())
        .send_to(&w.net)
        .await
        .unwrap()
        .assert_success();
    let before = w.keys(&agent.id).await;
    assert_eq!(before, vec![(alice_runner.public_key().to_string(), false)]);

    let bob_runner = generate_secret_key().unwrap();
    buy(
        &w,
        &xusd,
        &agent.id,
        &bob,
        bob_runner.public_key().to_string(),
    )
    .await
    .unwrap();

    let after = w.keys(&agent.id).await;
    println!("S5 keys before: {before:?}\nS5 keys after:  {after:?}");
    assert_eq!(after, vec![(bob_runner.public_key().to_string(), false)]);
    let deed = w
        .view(&agent.id, "nft_token", json!({"token_id": "deed"}))
        .await;
    assert_eq!(deed["owner_id"], bob.id.to_string());
    let alice_bal = w
        .view(&xusd, "ft_balance_of", json!({"account_id": alice.id}))
        .await;
    assert_eq!(alice_bal, json!((700 * USD).to_string()));
    let st = w.view(&agent.id, "get_status", json!({})).await;
    assert_eq!(st["escrow_left"], (300 * USD).to_string());
    assert_eq!(st["owner_epoch"], 2);

    // A17: the old runner key signs a runner call on the agent.
    let old = Acc {
        id: agent.id.clone(),
        sk: alice_runner.clone(),
        signer: Signer::from_secret_key(alice_runner).unwrap(),
    };
    let e = w
        .call(&old, &agent.id, "heartbeat", json!({}), 0)
        .await
        .unwrap_err();
    println!("A17 real error: {e}");
    assert!(is_missing_key(&e), "{e}");
    // A21: the setup full-access key deleted before listing.
    let e = w
        .call(&agent, &agent.id, "heartbeat", json!({}), 0)
        .await
        .unwrap_err();
    println!("A21 real error: {e}");
    assert!(is_missing_key(&e), "{e}");
    // bob's new runner key works.
    let new = Acc {
        id: agent.id.clone(),
        sk: bob_runner.clone(),
        signer: Signer::from_secret_key(bob_runner).unwrap(),
    };
    let ok = w
        .call(&new, &agent.id, "heartbeat", json!({}), 0)
        .await
        .unwrap();
    assert_eq!(ok["ok"], true);
}

#[tokio::test]
async fn e5_rollback_refunds_buyer() {
    let (w, xusd, agent, alice, bob, alice_runner, setup) = listed().await;
    // The buyer's key is already on the account (the setup key is still
    // there), so add_key fails and the whole batch reverts.
    let before = w.keys(&agent.id).await;
    let _ = buy(&w, &xusd, &agent.id, &bob, setup.public_key().to_string()).await;
    assert_eq!(w.keys(&agent.id).await, before, "key list unchanged");
    let bob_bal = w
        .view(&xusd, "ft_balance_of", json!({"account_id": bob.id}))
        .await;
    assert_eq!(
        bob_bal,
        json!((1_000 * USD).to_string()),
        "bob refunded in full"
    );
    let alice_bal = w
        .view(&xusd, "ft_balance_of", json!({"account_id": alice.id}))
        .await;
    assert_eq!(alice_bal, json!("0"), "no seller payout");
    let st = w.view(&agent.id, "get_status", json!({})).await;
    assert_eq!(st["status"], "Listed");
    assert_eq!(st["owner"], alice.id.to_string());
    assert!(st["known_keys"]
        .as_array()
        .unwrap()
        .contains(&json!(alice_runner.public_key().to_string())));
}

/// One payer: created by `batcher`, takes 1.00 from the faucet, pays it to
/// the agent. Sequential within the payer, concurrent across batchers.
async fn one_payer(
    net: NetworkConfig,
    batcher: AccountId,
    batcher_signer: Arc<Signer>,
    xusd: AccountId,
    agent: AccountId,
    label: String,
) -> Result<(), String> {
    let id: AccountId = format!("{label}.{batcher}").parse().map_err(|e| format!("{e:?}"))?;
    assert_eq!(id.as_str().len(), 64, "{id}");
    let sk = generate_secret_key().unwrap();
    Account::create_account(id.clone())
        .fund_myself(batcher, NearToken::from_millinear(50))
        .with_public_key(sk.public_key())
        .with_signer(batcher_signer)
        .send_to(&net)
        .await
        .map_err(|e| format!("create {id}: {e:?}"))?
        .assert_success();
    let signer = Signer::from_secret_key(sk).unwrap();
    for (method, args, yocto) in [
        ("faucet", json!({"amount": USD.to_string()}), 0u128),
        (
            "ft_transfer_call",
            json!({"receiver_id": agent, "amount": USD.to_string(), "msg": ""}),
            1,
        ),
    ] {
        let r = Contract(xusd.clone())
            .call_function(method, args)
            .transaction()
            .gas(NearGas::from_tgas(100))
            .deposit(NearToken::from_yoctonear(yocto))
            .with_signer(id.clone(), signer.clone())
            .send_to(&net)
            .await
            .map_err(|e| format!("{method} {id}: {e:?}"))?;
        r.into_result().map_err(|e| format!("{method} {id}: {e:?}"))?;
    }
    Ok(())
}

/// A47 (local-node finding at 12d1ab7: about 584 distinct 64-character
/// payers made every call trap while loading state). 1,000 distinct
/// 64-character payers pay 1.00 each; then every view still works, the
/// agent is bought, and the new owner's runner key works.
#[tokio::test]
async fn a47_thousand_long_payers_then_buy_and_views() {
    const PAYERS: usize = 1_000;
    const BATCHERS: usize = 50;
    let (w, xusd, agent, alice, bob, _alice_runner, _setup) = listed().await;
    let t0 = std::time::Instant::now();
    let mut batchers = vec![];
    for b in 0..BATCHERS {
        batchers.push(w.account(&format!("b{b:02}"), 2 + (PAYERS / BATCHERS) as u128 / 10).await);
    }
    let mut set = tokio::task::JoinSet::new();
    for (b, batcher) in batchers.iter().enumerate() {
        let (net, bid, bsig) = (w.net.clone(), batcher.id.clone(), batcher.signer.clone());
        let (xusd, agent_id) = (xusd.clone(), agent.id.clone());
        set.spawn(async move {
            let room = 64 - 1 - bid.as_str().len();
            for i in 0..PAYERS / BATCHERS {
                let label = format!("{:0>room$}", b * 1_000 + i);
                one_payer(net.clone(), bid.clone(), bsig.clone(), xusd.clone(), agent_id.clone(), label)
                    .await?;
            }
            Ok::<(), String>(())
        });
    }
    while let Some(r) = set.join_next().await {
        r.expect("task").expect("payer");
    }
    println!("A47: {PAYERS} payers paid in {:?}", t0.elapsed());

    for (method, args) in [
        ("get_status", json!({})),
        ("get_earnings", json!({})),
        ("get_earnings_days", json!({})),
        ("get_listing", json!({})),
        ("nft_token", json!({"token_id": "deed"})),
    ] {
        let v = w.view(&agent.id, method, args).await;
        assert!(!v.is_null(), "{method}");
    }
    let e = w.view(&agent.id, "get_earnings", json!({})).await;
    assert_eq!(e["tracked_payers"], PAYERS as u64 + 1, "p1 plus the 1,000");
    let st = w.view(&agent.id, "get_status", json!({})).await;
    println!("A47 near_storage: {}", st["near_storage"]);
    assert_eq!(st["near_storage"]["solvent"], true);

    let bob_runner = generate_secret_key().unwrap();
    buy(&w, &xusd, &agent.id, &bob, bob_runner.public_key().to_string())
        .await
        .unwrap();
    let deed = w.view(&agent.id, "nft_token", json!({"token_id": "deed"})).await;
    assert_eq!(deed["owner_id"], bob.id.to_string());
    let alice_bal = w.view(&xusd, "ft_balance_of", json!({"account_id": alice.id})).await;
    assert_eq!(alice_bal, json!((700 * USD).to_string()));
    let new = Acc {
        id: agent.id.clone(),
        sk: bob_runner.clone(),
        signer: Signer::from_secret_key(bob_runner).unwrap(),
    };
    let ok = w.call(&new, &agent.id, "heartbeat", json!({}), 0).await.unwrap();
    assert_eq!(ok["ok"], true);
    let st = w.view(&agent.id, "get_status", json!({})).await;
    assert_eq!(st["owner"], bob.id.to_string());
}
