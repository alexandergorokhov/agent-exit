//! F1-F7: the born-clean factory (mocked runtime: the batch's actions are
//! checked, the callback is driven by hand).
use super::*;
use near_sdk::mock::MockAction;
use near_sdk::test_utils::{get_created_receipts, get_logs, VMContextBuilder};
use near_sdk::testing_env;
use near_sdk::PromiseResult;

const FACTORY: &str = "agentexit.testnet";
const ALICE: &str = "alice.testnet";

fn ctx(pred: &str, deposit: NearToken) {
    testing_env!(VMContextBuilder::new()
        .current_account_id(FACTORY.parse().unwrap())
        .predecessor_account_id(pred.parse().unwrap())
        .attached_deposit(deposit)
        .block_timestamp(1_770_000_000_000_000_000)
        .block_height(42)
        .build());
}

fn with_input(bytes: &[u8]) {
    let mut c = VMContextBuilder::new()
        .current_account_id(FACTORY.parse().unwrap())
        .predecessor_account_id(FACTORY.parse().unwrap())
        .build();
    c.input = bytes.into();
    testing_env!(c);
}

const CODE: &[u8] = b"\0asm fake agent code for the tests";

fn factory() -> Factory {
    ctx(FACTORY, NearToken::from_near(0));
    let hash: Base58CryptoHash = env::sha256_array(CODE).into();
    let mut f = Factory::new(hash);
    with_input(CODE);
    f.store_code();
    f
}

fn callback(f: &mut Factory, agent: &str, ok: bool) -> PromiseOrValue<bool> {
    let r = if ok {
        PromiseResult::Successful(vec![])
    } else {
        PromiseResult::Failed
    };
    testing_env!(
        VMContextBuilder::new()
            .current_account_id(FACTORY.parse().unwrap())
            .predecessor_account_id(FACTORY.parse().unwrap())
            .build(),
        near_sdk::test_vm_config(),
        near_sdk::RuntimeFeesConfig::test(),
        Default::default(),
        vec![r]
    );
    f.on_created(agent.parse().unwrap())
}

#[test]
fn bc1_one_batch_creates_funds_uses_published_code_and_inits_with_no_key() {
    let mut f = factory();
    ctx(ALICE, NearToken::from_near(9));
    let _ = f.create_agent("translator".into(), None, None);
    let receipts = get_created_receipts();
    let r = receipts
        .iter()
        .find(|r| r.receiver_id.as_str() == "translator.agentexit.testnet")
        .expect("one receipt to the new agent");
    let mut saw = (false, false, false, false);
    for a in &r.actions {
        match a {
            MockAction::CreateAccount { .. } => saw.0 = true,
            MockAction::Transfer { deposit, .. } => {
                assert_eq!(*deposit, NearToken::from_near(9));
                saw.1 = true;
            }
            MockAction::DeployContract { code, .. } => {
                assert_eq!(code.as_slice(), CODE);
                saw.2 = true;
            }
            MockAction::FunctionCallWeight {
                method_name, args, ..
            } => {
                assert_eq!(method_name, b"new");
                let v: serde_json::Value = serde_json::from_slice(args).unwrap();
                assert_eq!(v["owner_id"], ALICE);
                saw.3 = true;
            }
            MockAction::AddKeyWithFullAccess { .. } | MockAction::AddKeyWithFunctionCall { .. } => {
                panic!("the factory must never add a key: {a:?}")
            }
            other => panic!("unexpected action {other:?}"),
        }
    }
    assert_eq!(saw, (true, true, true, true));
    let b = f
        .get_agent("translator.agentexit.testnet".parse().unwrap())
        .unwrap();
    assert_eq!(b.state, BirthState::Creating);
    assert_eq!(b.owner_at_birth.as_str(), ALICE);
    assert_eq!(b.code_hash, Base58CryptoHash::from(env::sha256_array(CODE)));
    assert_eq!(f.total_agents(), 0, "listed only after the batch succeeded");
}

#[test]
fn bc2_success_records_the_birth_and_emits_agent_born() {
    let mut f = factory();
    ctx(ALICE, NearToken::from_near(10));
    let _ = f.create_agent("translator".into(), None, None);
    assert!(matches!(
        callback(&mut f, "translator.agentexit.testnet", true),
        PromiseOrValue::Value(true)
    ));
    let b = f
        .get_agent("translator.agentexit.testnet".parse().unwrap())
        .unwrap();
    assert_eq!(b.state, BirthState::Born);
    assert_eq!(b.block_height, U64(42));
    assert_eq!(f.total_agents(), 1);
    assert_eq!(f.list_agents(None, None), vec![b]);
    let logs = get_logs().join("\n");
    assert!(
        logs.contains("\"event\":\"agent_born\"") && logs.contains("\"access_keys_added\":0"),
        "{logs}"
    );
}

#[test]
fn bc3_failure_forgets_the_name_and_refunds_the_caller() {
    let mut f = factory();
    ctx(ALICE, NearToken::from_near(11));
    let _ = f.create_agent("translator".into(), None, None);
    let _ = callback(&mut f, "translator.agentexit.testnet", false);
    assert!(f
        .get_agent("translator.agentexit.testnet".parse().unwrap())
        .is_none());
    assert_eq!(f.total_agents(), 0);
    let refund = get_created_receipts()
        .into_iter()
        .find(|r| r.receiver_id.as_str() == ALICE)
        .expect("refund to alice");
    assert!(refund.actions.iter().any(
        |a| matches!(a, MockAction::Transfer { deposit, .. } if *deposit == NearToken::from_near(11))
    ));
    assert!(get_logs().join("").contains("agent_birth_failed"));
}

#[test]
#[should_panic(expected = "DepositTooSmall")]
fn bc4_needs_nine_near() {
    let mut f = factory();
    ctx(ALICE, NearToken::from_millinear(8999));
    let _ = f.create_agent("translator".into(), None, None);
}

#[test]
#[should_panic(expected = "NameTaken")]
fn bc5_a_name_is_born_once() {
    let mut f = factory();
    ctx(ALICE, NearToken::from_near(9));
    let _ = f.create_agent("translator".into(), None, None);
    ctx("mallory.testnet", NearToken::from_near(9));
    let _ = f.create_agent("translator".into(), None, None);
}

#[test]
fn bc6_names_are_one_level_and_plain() {
    for bad in [
        "a",
        "x.alice",
        "Upper",
        "-lead",
        "trail_",
        "sp ace",
        &"a".repeat(33),
    ] {
        assert!(!valid_name(bad), "{bad}");
    }
    for ok in ["translator", "t2", "my-agent", "my_agent_7"] {
        assert!(valid_name(ok), "{ok}");
    }
}

#[test]
#[should_panic(expected = "Method on_created is private")]
fn bc7_only_the_factory_can_record_a_birth() {
    let mut f = factory();
    ctx(ALICE, NearToken::from_near(9));
    let _ = f.create_agent("translator".into(), None, None);
    ctx(ALICE, NearToken::from_near(0));
    let _ = f.on_created("translator.agentexit.testnet".parse().unwrap());
}

#[test]
fn bc8_list_pages() {
    let mut f = factory();
    for i in 0..5 {
        ctx(ALICE, NearToken::from_near(9));
        let name = format!("agent{i}");
        let _ = f.create_agent(name.clone(), None, None);
        let _ = callback(&mut f, &format!("{name}.agentexit.testnet"), true);
    }
    let page: Vec<String> = f
        .list_agents(Some(1), Some(2))
        .into_iter()
        .map(|b| b.agent.to_string())
        .collect();
    assert_eq!(
        page,
        vec!["agent1.agentexit.testnet", "agent2.agentexit.testnet"]
    );
    assert_eq!(f.list_agents(Some(9), None).len(), 0);
}

#[test]
#[should_panic(expected = "CodeHashMismatch")]
fn bc9_only_the_published_build_is_stored() {
    let mut f = factory();
    with_input(b"\0asm patched code");
    f.store_code();
}

#[test]
#[should_panic(expected = "NoCode")]
fn bc10_no_agent_before_the_code_is_stored() {
    ctx(FACTORY, NearToken::from_near(0));
    let mut f = Factory::new(env::sha256_array(CODE).into());
    ctx(ALICE, NearToken::from_near(9));
    let _ = f.create_agent("translator".into(), None, None);
}

#[test]
fn bc11_a_platform_can_create_an_agent_for_someone_else() {
    let mut f = factory();
    ctx(FACTORY, NearToken::from_near(9));
    let _ = f.create_agent("gift".into(), Some(ALICE.parse().unwrap()), None);
    let new = call_args_new();
    assert_eq!(new["owner_id"], ALICE);
    let b = f
        .get_agent("gift.agentexit.testnet".parse().unwrap())
        .unwrap();
    assert_eq!(b.owner_at_birth.as_str(), ALICE);
}

fn call_args_new() -> serde_json::Value {
    for r in get_created_receipts() {
        for a in r.actions {
            if let MockAction::FunctionCallWeight {
                method_name, args, ..
            } = a
            {
                if method_name == b"new" {
                    return serde_json::from_slice(&args).unwrap();
                }
            }
        }
    }
    panic!("no new call")
}
