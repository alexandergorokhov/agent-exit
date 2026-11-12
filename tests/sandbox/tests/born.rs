//! BC-S1: born clean on a real local node. The factory creates an agent in
//! one batch (create account, fund, deploy the published code, run `new`):
//! the agent starts with no access key at all, the factory records the birth,
//! and a stranger cannot run `new` (security review 6 H1: the agent's init
//! accepts its direct parent, the factory).

use near_api::signer::generate_secret_key;
use near_api::types::{AccountId, CryptoHash, NearGas, NearToken};
use near_api::{Account, Contract, NetworkConfig, Signer};
use near_sandbox::config::{DEFAULT_GENESIS_ACCOUNT, DEFAULT_GENESIS_ACCOUNT_PRIVATE_KEY};
use serde_json::{json, Value};
use std::sync::Arc;

fn wasm(var: &str, default: &str) -> Vec<u8> {
    let p = std::env::var(var).unwrap_or_else(|_| format!("../../target/near-sandbox/{default}"));
    std::fs::read(&p).unwrap_or_else(|e| panic!("{p}: {e} (run `make test-sandbox`)"))
}

#[allow(dead_code)]
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

    #[allow(dead_code)]
    async fn ft(&self, xusd: &AccountId, who: &AccountId) -> u128 {
        self.view(xusd, "ft_balance_of", json!({"account_id": who}))
            .await
            .as_str()
            .unwrap()
            .parse()
            .unwrap()
    }
}

#[tokio::test]
async fn bc_s1_factory_births_an_agent_with_no_key() {
    let w = World::start().await;
    let agent_wasm = wasm("AGENT_WASM", "agent_exit.wasm");
    let code_hash = CryptoHash::hash(&agent_wasm).to_string();
    let factory = w.account("factory", 40).await;
    let alice = w.account("alice", 30).await;
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
        .call_function_raw("store_code", agent_wasm.clone())
        .transaction()
        .gas(NearGas::from_tgas(300))
        .with_signer(factory.id.clone(), factory.signer.clone())
        .send_to(&w.net)
        .await
        .unwrap()
        .assert_success();
    assert_eq!(
        w.view(&factory.id, "has_code", json!({})).await,
        json!(true)
    );

    w.ok(
        &alice,
        &factory.id,
        "create_agent",
        json!({"name": "born1"}),
        10_200 * 10u128.pow(21),
    )
    .await;
    let agent: AccountId = format!("born1.{}", factory.id).parse().unwrap();
    let birth = w
        .view(&factory.id, "get_agent", json!({"account_id": agent}))
        .await;
    assert_eq!(birth["state"], json!("Born"), "{birth}");
    assert_eq!(birth["code_hash"], json!(code_hash));
    let keys = Account(agent.clone())
        .list_keys()
        .fetch_from(&w.net)
        .await
        .unwrap();
    assert!(keys.data.is_empty(), "born with no key: {:?}", keys.data);
    let st = w.view(&agent, "get_status", json!({})).await;
    assert_eq!(st["owner"], json!(alice.id));

    // A stranger cannot initialise an account that is not its own child.
    let lone = w.account("lone", 20).await;
    Contract::deploy(lone.id.clone())
        .use_code(agent_wasm)
        .without_init_call()
        .with_signer(lone.signer.clone())
        .send_to(&w.net)
        .await
        .unwrap()
        .assert_success();
    let (r, _) = w
        .call(&alice, &lone.id, "new", json!({"owner_id": alice.id}), 0)
        .await;
    assert!(
        r.as_ref().is_err_and(|e| e.contains("InitNotAllowed")),
        "{r:?}"
    );
}

/// BC-S2: a plain deploy (not created by the pinned factory) cannot be listed
/// or bought: the contract answers NotBornClean, even though the owner's own
/// key could initialise it.
#[tokio::test]
async fn bc_s2_plain_deploy_cannot_be_listed() {
    let w = World::start().await;
    let alice = w.account("alice", 30).await;
    let plain = w.account("translator", 20).await;
    Contract::deploy(plain.id.clone())
        .use_code(wasm("AGENT_WASM", "agent_exit.wasm"))
        .with_init_call("new", json!({"owner_id": alice.id}))
        .unwrap()
        .with_signer(plain.signer.clone())
        .send_to(&w.net)
        .await
        .unwrap()
        .assert_success();
    let st = w.view(&plain.id, "get_status", json!({})).await;
    assert_eq!(st["born_clean"], json!(false), "{st}");
    let (r, _) = w
        .call(
            &alice,
            &plain.id,
            "list_for_sale",
            json!({"price": "1000000000", "code_hash": "sandbox"}),
            1,
        )
        .await;
    assert!(
        r.as_ref().is_err_and(|e| e.contains("NotBornClean")),
        "{r:?}"
    );
}
