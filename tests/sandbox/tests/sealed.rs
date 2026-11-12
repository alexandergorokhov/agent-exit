//! SC-S1: the sealed-config dispute on a real local node, with gas.
//! The seller sells a sealed config, posts a FAKE bundle, the buyer reveals
//! the one-time key and the contract decrypts on-chain: the whole price goes
//! back to the buyer, the seller gets nothing. An honest delivery (SC-S2) is
//! refused as a dispute and accepted. Gas per call is printed and capped.

use agent_core::sealed::{public_key, seal, to_hex, ConfigCommit};
use near_api::signer::generate_secret_key;
use near_api::types::{AccountId, NearGas, NearToken};
use near_api::{Account, Contract, NetworkConfig, Signer};
use near_sandbox::config::{DEFAULT_GENESIS_ACCOUNT, DEFAULT_GENESIS_ACCOUNT_PRIVATE_KEY};
use serde_json::{json, Value};
use std::sync::Arc;

const USD: u128 = 1_000_000;
/// Measured 19 Tgas for the worst case (8 KB fake bundle); 30 is the gate.
const MAX_TGAS: u64 = 30;

fn wasm(var: &str, default: &str) -> Vec<u8> {
    let p = std::env::var(var).unwrap_or_else(|_| format!("../../target/near-sandbox/{default}"));
    std::fs::read(&p).unwrap_or_else(|e| panic!("{p}: {e} (run `make test-sandbox`)"))
}

fn b64(data: &[u8]) -> String {
    const T: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut s = String::new();
    for c in data.chunks(3) {
        let n = (c[0] as u32) << 16
            | (*c.get(1).unwrap_or(&0) as u32) << 8
            | *c.get(2).unwrap_or(&0) as u32;
        for i in 0..4 {
            if i <= c.len() {
                s.push(T[(n >> (18 - 6 * i) & 63) as usize] as char);
            } else {
                s.push('=');
            }
        }
    }
    s
}

struct Acc {
    id: AccountId,
    signer: Arc<Signer>,
}

struct World {
    _sbx: near_sandbox::Sandbox,
    net: NetworkConfig,
    root: AccountId,
    root_signer: Arc<Signer>,
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
        Acc {
            id,
            signer: Signer::from_secret_key(sk).unwrap(),
        }
    }

    /// Call and return (result, Tgas burnt by the whole transaction).
    async fn call(
        &self,
        who: &Acc,
        contract: &AccountId,
        method: &str,
        args: Value,
        yocto: u128,
    ) -> (Result<Value, String>, u64) {
        let r = Contract(contract.clone())
            .call_function(method, args)
            .transaction()
            .gas(NearGas::from_tgas(300))
            .deposit(NearToken::from_yoctonear(yocto))
            .with_signer(who.id.clone(), who.signer.clone())
            .send_to(&self.net)
            .await
            .expect("send");
        const T: u64 = 1_000_000_000_000;
        match r.into_result() {
            Ok(ok) => {
                let tgas = ok.total_gas_burnt.as_gas().div_ceil(T);
                (Ok(ok.json::<Value>().unwrap_or(Value::Null)), tgas)
            }
            Err(near_api::types::transaction::result::TransactionResultError::Failure(f)) => (
                Err(format!("{f:?}")),
                f.total_gas_burnt.as_gas().div_ceil(T),
            ),
            Err(e) => (Err(format!("{e:?}")), 0),
        }
    }

    async fn ok(
        &self,
        who: &Acc,
        contract: &AccountId,
        method: &str,
        args: Value,
        yocto: u128,
    ) -> Value {
        let (r, tgas) = self.call(who, contract, method, args, yocto).await;
        println!("GAS {method}: {tgas} Tgas");
        r.unwrap_or_else(|e| panic!("{method}: {e}"))
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

    async fn ft(&self, xusd: &AccountId, who: &AccountId) -> u128 {
        self.view(xusd, "ft_balance_of", json!({"account_id": who}))
            .await
            .as_str()
            .unwrap()
            .parse()
            .unwrap()
    }
}

/// A bundle like a real one (prompt, tool setup, keys to rotate), `len` bytes.
fn bundle(tag: &str, len: usize) -> Vec<u8> {
    let mut b = format!("{{\"system_prompt\":\"You are a {tag} translator.\",\"tools\":[\"glossary\"],\"rotate\":[\"OPENAI_KEY\"],\"pad\":\"").into_bytes();
    b.resize(len - 2, b'x');
    b.extend_from_slice(b"\"}");
    b
}

/// Deploy xusd + the agent, one payment, commit to `honest`, list at 1,000,
/// bob buys with a one-time delivery key. Returns the world and the sale id.
async fn sold(
    honest: &[u8],
    delivery_sk: &[u8; 32],
) -> (World, AccountId, AccountId, Acc, Acc, u64) {
    let w = World::start().await;
    let xusd = w.account("xusd", 20).await;
    let factory = w.account("factory", 40).await;
    let alice = w.account("alice", 40).await;
    let never_sk = generate_secret_key().unwrap();
    let agent = Acc {
        id: format!("translator.{}", factory.id).parse().unwrap(),
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
        (&bob.id, 1_000 * USD),
        (&p1.id, 100 * USD),
        (&alice.id, 0),
        (&agent.id, 0),
    ] {
        w.ok(
            &xusd,
            &xusd.id,
            "mint",
            json!({"account_id": who, "amount": amt.to_string()}),
            0,
        )
        .await;
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
    w.ok(
        &alice,
        &factory.id,
        "create_agent",
        json!({"name": "translator"}),
        30_000 * 10u128.pow(21),
    )
    .await;
    let runner = generate_secret_key().unwrap();
    w.ok(
        &alice,
        &agent.id,
        "set_runner_key",
        json!({"public_key": runner.public_key().to_string()}),
        1,
    )
    .await;
    w.ok(
        &p1,
        &xusd.id,
        "ft_transfer_call",
        json!({"receiver_id": agent.id, "amount": (12 * USD).to_string(), "msg": ""}),
        1,
    )
    .await;
    let c = ConfigCommit::of(honest);
    w.ok(
        &alice,
        &agent.id,
        "commit_config",
        json!({"config_hash": c.hash, "bytes": c.bytes, "chunks_root": c.root}),
        1,
    )
    .await;
    w.ok(
        &alice,
        &agent.id,
        "list_for_sale",
        json!({"price": (1_000 * USD).to_string(), "code_hash": "sandbox"}),
        1,
    )
    .await;

    // SC21: bob asks for random chunks; the block seed picks them; alice
    // proves each against the committed root before bob buys.
    // Review 5 P2: a request pays its storage (0.01 NEAR) and is bob's own.
    let r = w
        .ok(
            &bob,
            &agent.id,
            "request_preview",
            json!({}),
            10u128.pow(22),
        )
        .await;
    let idx: Vec<u32> = serde_json::from_value(r["chunks"].clone()).unwrap();
    assert_eq!(
        idx.len() as u32,
        agent_core::sealed::preview_size(honest.len() as u32),
        "{r}"
    );
    for i in &idx {
        let (chunk, proof) = agent_core::sealed::chunk_proof(honest, *i).unwrap();
        w.ok(
            &alice,
            &agent.id,
            "answer_preview",
            json!({"requester": bob.id, "index": i, "chunk": b64(&chunk), "proof": proof}),
            1,
        )
        .await;
    }
    let pv = w
        .view(&agent.id, "get_preview", json!({"requester": bob.id}))
        .await;
    assert_eq!(pv["complete"], json!(true), "{pv}");
    let (bad, _) = w
        .call(
            &alice,
            &agent.id,
            "answer_preview",
            json!({"requester": bob.id, "index": idx[0], "chunk": b64(b"forged"), "proof": []}),
            1,
        )
        .await;
    assert!(bad.is_err(), "a forged chunk is refused");
    let st = w.view(&agent.id, "get_status", json!({})).await;
    let buyer_runner = generate_secret_key().unwrap();
    let msg = json!({"kind": "buy", "listing_version": st["listing_version"],
        "known_keys_hash": st["known_keys_hash"], "buyer_runner_pk": buyer_runner.public_key().to_string(),
        "delivery_pk": to_hex(&public_key(delivery_sk))});
    w.ok(&bob, &xusd.id, "ft_transfer_call",
        json!({"receiver_id": agent.id, "amount": (1_000 * USD).to_string(), "msg": msg.to_string()}), 1).await;
    assert_eq!(w.ft(&xusd.id, &bob.id).await, 0, "bob paid");
    let sales = w.view(&agent.id, "get_sales", json!({})).await;
    let sale_id = sales.as_array().unwrap().last().unwrap()["sale_id"]
        .as_u64()
        .expect("sale id");
    let sc = w
        .view(&agent.id, "get_sealed_config", json!({"sale_id": sale_id}))
        .await;
    assert_eq!(
        sc["cleared"],
        json!(false),
        "upfront held until the bundle clears: {sc}"
    );
    (w, xusd.id, agent.id, alice, bob, sale_id)
}

async fn deliver(
    w: &World,
    agent: &AccountId,
    alice: &Acc,
    sale_id: u64,
    delivery_sk: &[u8; 32],
    what: &[u8],
) {
    let (eph_pk, cipher) = seal(&[17u8; 32], &public_key(delivery_sk), what).unwrap();
    // Storage for the ciphertext: (len + 200) bytes at 10^19 yocto per byte.
    let deposit = 10u128.pow(19) * (cipher.len() as u128 + 200);
    w.ok(
        alice,
        agent,
        "deliver_config",
        json!({"sale_id": sale_id, "ephemeral_pk": to_hex(&eph_pk), "ciphertext": b64(&cipher)}),
        deposit,
    )
    .await;
}

#[tokio::test]
async fn sc_s1_fake_bundle_refunds_the_whole_price() {
    // The largest bundle the contract takes (8 KB): the worst-case dispute.
    let sk = [42u8; 32];
    let honest = bundle("Spanish", agent_core::sealed::MAX_CONFIG_BYTES as usize);
    let (w, xusd, agent, alice, bob, sale_id) = sold(&honest, &sk).await;
    // The seller posts a different bundle, same length.
    deliver(
        &w,
        &agent,
        &alice,
        sale_id,
        &sk,
        &bundle("useless", honest.len()),
    )
    .await;

    let (r, tgas) = w
        .call(
            &bob,
            &agent,
            "dispute_config",
            json!({"sale_id": sale_id, "delivery_sk": to_hex(&sk)}),
            0,
        )
        .await;
    println!("GAS dispute_config (fake 8 KB bundle, decrypted on-chain): {tgas} Tgas");
    let r = r.expect("dispute of a fake bundle succeeds");
    assert_eq!(
        r["refund_to_buyer"],
        json!((1_000 * USD).to_string()),
        "{r}"
    );
    assert!(tgas <= MAX_TGAS, "dispute burnt {tgas} Tgas");

    w.ok(&bob, &agent, "claim", json!({}), 0).await;
    assert_eq!(
        w.ft(&xusd, &bob.id).await,
        1_000 * USD,
        "bob gets the whole price back"
    );
    assert_eq!(
        w.ft(&xusd, &alice.id).await,
        0,
        "the seller gets nothing of the price"
    );
    let sc = w
        .view(&agent, "get_sealed_config", json!({"sale_id": sale_id}))
        .await;
    assert!(sc["state"].to_string().contains("Mismatch"), "{sc}");
    // The seller cannot take the upfront part afterwards.
    let (again, _) = w
        .call(
            &alice,
            &agent,
            "release_upfront",
            json!({"sale_id": sale_id}),
            0,
        )
        .await;
    assert!(again.is_err(), "release after a won dispute is refused");
}

#[tokio::test]
async fn sc_s2_honest_bundle_dispute_refused_then_accepted() {
    let sk = [42u8; 32];
    let honest = bundle("Spanish", 2_000);
    let (w, xusd, agent, alice, bob, sale_id) = sold(&honest, &sk).await;
    deliver(&w, &agent, &alice, sale_id, &sk, &honest).await;

    let (r, tgas) = w
        .call(
            &bob,
            &agent,
            "dispute_config",
            json!({"sale_id": sale_id, "delivery_sk": to_hex(&sk)}),
            0,
        )
        .await;
    println!("GAS dispute_config (honest bundle, refused): {tgas} Tgas");
    assert!(r.is_err(), "a matching bundle refuses the dispute");

    w.ok(
        &bob,
        &agent,
        "release_upfront",
        json!({"sale_id": sale_id}),
        0,
    )
    .await;
    let sc = w
        .view(&agent, "get_sealed_config", json!({"sale_id": sale_id}))
        .await;
    assert_eq!(sc["cleared"], json!(true), "{sc}");
    w.ok(&alice, &agent, "claim", json!({}), 0).await;
    let got = w.ft(&xusd, &alice.id).await;
    assert!(
        got > 0 && got < 1_000 * USD,
        "seller paid the upfront part, earn-out still held: {got}"
    );
}
