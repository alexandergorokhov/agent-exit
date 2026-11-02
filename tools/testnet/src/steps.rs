//! The steps of `testnet_plan::plan`, run against testnet. Each step is
//! idempotent where it can be (skips what is already done), records every
//! transaction, and fails loudly with the node's real error.

use crate::chain::*;
use near_api::NetworkConfig;
use serde_json::{json, Value};
use std::collections::BTreeMap;
use std::time::{Duration, Instant};
use testnet_plan::*;

const USD: u128 = 1_000_000;
const ROOT_DIR: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../..");

/// Everything one command records, written to out/ at the end (and after
/// each step, so a failure still leaves a record).
struct Out {
    file: String,
    v: BTreeMap<String, Value>,
    secrets: Vec<String>,
}

impl Out {
    fn new(file: &str, secrets: Vec<String>) -> Out {
        Out {
            file: format!("{ROOT_DIR}/{file}"),
            v: BTreeMap::new(),
            secrets,
        }
    }

    fn set(&mut self, k: &str, v: Value) {
        self.v.insert(k.into(), v);
    }

    fn push(&mut self, k: &str, v: Value) {
        let e = self.v.entry(k.into()).or_insert_with(|| json!([]));
        if let Some(a) = e.as_array_mut() {
            a.push(v);
        }
    }

    fn tx(&mut self, t: &Tx) {
        self.push("txs", t.json());
    }

    fn write(&self) -> Res<()> {
        let text = serde_json::to_string_pretty(&self.v).map_err(|e| e.to_string())?;
        // Never write a secret into out/ (it is meant to be committed).
        if self
            .secrets
            .iter()
            .any(|s| s.len() >= 8 && text.contains(s.as_str()))
        {
            return Err(format!(
                "refusing to write {}: it would contain a secret",
                self.file
            ));
        }
        if let Some(dir) = std::path::Path::new(&self.file).parent() {
            std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
        }
        std::fs::write(&self.file, text + "\n").map_err(|e| format!("{}: {e}", self.file))?;
        println!(
            "  wrote {}",
            self.file.trim_start_matches(&format!("{ROOT_DIR}/"))
        );
        Ok(())
    }
}

fn wasm(var: &str, default: &str) -> Res<Vec<u8>> {
    let path =
        std::env::var(var).unwrap_or_else(|_| format!("{ROOT_DIR}/target/near-testnet/{default}"));
    std::fs::read(&path).map_err(|e| format!("{path}: {e} (run make build-testnet first)"))
}

fn ok(t: &Tx) -> Res<()> {
    if t.ok {
        Ok(())
    } else {
        Err(format!("{} failed: {}", t.label, t.err()))
    }
}

fn now_ns() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos() as u64)
        .unwrap_or(0)
}

fn demo_limits(summarizer: &str, base: Option<(u128, &str)>) -> Value {
    let mut l = json!({
        "price_floor": (5 * USD).to_string(), "daily_cap": (50 * USD).to_string(),
        "allowed_payees": [summarizer], "split": {"owner_bps": 8000, "treasury_bps": 2000},
    });
    if let Some((cap, payee)) = base {
        l["base"] = json!({"daily_cap": cap.to_string(), "payees": [payee]});
    }
    json!({"limits": l})
}

pub async fn run(cmd: Command, cfg: Config, env: BTreeMap<String, String>) -> Res<()> {
    let root_sk = env
        .get("AGENT_EXIT_ROOT_KEY")
        .filter(|v| !v.is_empty())
        .ok_or(
            "AGENT_EXIT_ROOT_KEY missing in the env file (the full-access key of AGENT_EXIT_ROOT)",
        )?
        .parse()
        .map_err(|_| "AGENT_EXIT_ROOT_KEY is not a NEAR secret key (ed25519:...)".to_string())?;
    let root = acct(&cfg.root, &root_sk)?;
    let net =
        NetworkConfig::from_rpc_url("testnet", cfg.rpc[0].parse().map_err(|e| format!("{e:?}"))?);
    let net2 =
        NetworkConfig::from_rpc_url("testnet", cfg.rpc[1].parse().map_err(|e| format!("{e:?}"))?);
    let keys = KeyStore {
        dir: cfg.keys_dir.clone(),
    };
    let mut secrets: Vec<String> = SECRET_VARS
        .iter()
        .filter_map(|k| env.get(*k).cloned())
        .collect();
    secrets.extend(keys.all_secrets());
    let c = Chain {
        cfg,
        net,
        net2,
        keys,
        root,
    };
    match cmd {
        Command::Deploy => deploy(&c, secrets).await,
        Command::GateG1 => g1(&c, secrets).await,
        Command::GateG2 => g2(&c, &env, secrets).await,
        Command::GateG6 => g6(&c, &env, secrets).await,
        Command::Gates => {
            let mut failed = vec![];
            if let Err(e) = g1(&c, secrets.clone()).await {
                failed.push(format!("G1: {e}"));
            }
            if let Err(e) = g6(&c, &env, secrets.clone()).await {
                failed.push(format!("G6: {e}"));
            }
            if let Err(e) = g2(&c, &env, secrets).await {
                failed.push(format!("G2: {e}"));
            }
            if failed.is_empty() {
                Ok(())
            } else {
                Err(failed.join("; "))
            }
        }
        Command::Scenes => {
            for n in RECORDING_ORDER {
                scene(&c, n, secrets.clone()).await?;
            }
            Ok(())
        }
        Command::Scene(n) => scene(&c, n, secrets).await,
    }
}

// ------------------------------------------------------------------ deploy

async fn deploy(c: &Chain, secrets: Vec<String>) -> Res<()> {
    let mut out = Out::new("out/testnet/deploy.json", secrets);
    out.set("schema", json!("testnet-deploy.v1"));
    out.set("root", json!(c.cfg.root));
    let agent_wasm = wasm("AGENT_WASM", "agent_exit.wasm")?;
    let xusd_wasm = wasm("XUSD_WASM", "xusd.wasm")?;
    let agent_hash = sha256_b58(&agent_wasm);
    out.set("agent_code_hash", json!(agent_hash));

    println!("check-root");
    let need: u128 = FUNDING.iter().map(|(_, n)| *n as u128).sum::<u128>() * 10u128.pow(24);
    let have = c.balance_near(&c.cfg.root).await?;
    let keys = c.keys(&c.cfg.root).await?;
    if !keys.iter().any(|k| k.public_key == c.root.pk) {
        return Err(format!(
            "AGENT_EXIT_ROOT_KEY is not a key of {}",
            c.cfg.root
        ));
    }
    let created_already = c.exists(&c.id("translator")).await;
    if !created_already && have < need {
        return Err(format!(
            "{} holds {} NEAR; the plan needs about {} (testnet faucet)",
            c.cfg.root,
            have / 10u128.pow(24),
            need / 10u128.pow(24)
        ));
    }

    println!("create-accounts");
    let mut accounts = BTreeMap::new();
    for (name, near) in FUNDING.iter().filter(|(n, _)| !n.contains(' ')) {
        let id = c.id(name);
        let parent = if *name == "x.alice" {
            c.acct("alice")?
        } else {
            c.root.clone()
        };
        if c.exists(&id).await {
            println!("  skip {id} (exists)");
        } else {
            let t = c
                .create(&format!("create {id}"), &parent, &id, name, *near)
                .await?;
            out.tx(&t);
            ok(&t)?;
        }
        accounts.insert(name.to_string(), json!(id));
    }
    out.set("accounts", json!(accounts));
    out.write()?;

    println!("deploy-xusd");
    let xusd = c.acct("xusd")?;
    let has_xusd = c.code(&c.id("xusd")).await.map(|(h, _)| h).ok();
    if has_xusd.as_deref() != Some(sha256_b58(&xusd_wasm).as_str()) {
        if has_xusd.is_some() && has_xusd.as_deref() != Some("11111111111111111111111111111111") {
            return Err("xusd already holds other code; not redeploying".into());
        }
        let t = c
            .deploy(
                "deploy xusd",
                &xusd,
                xusd_wasm,
                json!({"admin_id": c.cfg.root}),
            )
            .await?;
        out.tx(&t);
        ok(&t)?;
    }

    println!("mint-xusd");
    let targets: Vec<(String, u128)> = [
        ("bob", 2_000),
        ("carol", 2_000),
        ("summarizer", 2_000),
        ("alice", 300),
        ("x.alice", 300),
        ("p1", 200),
        ("p2", 200),
        ("p3", 200),
        ("p4", 200),
        ("p5", 200),
        ("p6", 200),
        ("translator", 0),
        ("cheat", 0),
    ]
    .iter()
    .map(|(n, a)| (c.id(n), a * USD))
    .collect();
    for (id, want) in targets {
        let bal: u128 = c
            .view(&c.id("xusd"), "ft_balance_of", json!({"account_id": id}))
            .await
            .ok()
            .and_then(|v| v.as_str().and_then(|s| s.parse().ok()))
            .unwrap_or(0);
        let registered = c
            .view(
                &c.id("xusd"),
                "storage_balance_of",
                json!({"account_id": id}),
            )
            .await
            .map(|v| !v.is_null())
            .unwrap_or(false);
        if bal >= want && registered {
            continue;
        }
        let t = c
            .call(
                &format!("mint {id}"),
                &c.root,
                &c.id("xusd"),
                "mint",
                json!({"account_id": id, "amount": want.saturating_sub(bal).to_string()}),
                0,
                30,
            )
            .await;
        out.tx(&t);
        ok(&t)?;
    }
    out.write()?;

    println!("derive-base");
    let mpc_pk = c.view(MPC_SIGNER, "public_key", json!({})).await?;
    let mpc_pk = mpc_pk
        .as_str()
        .ok_or("MPC public_key is not a string")?
        .to_string();
    let root_pk = base_wallet::parse_near_secp_pk(&mpc_pk)?;
    let mut base = BTreeMap::new();
    for a in ["translator", "cheat"] {
        base.insert(
            a.to_string(),
            base_wallet::derive_address(&root_pk, &c.id(a), agent_core::base::BASE_PATH),
        );
    }
    out.set("mpc", json!({"signer": MPC_SIGNER, "public_key": mpc_pk}));
    out.set("base_addresses", json!(base));

    println!("deploy-agents");
    for a in ["translator", "cheat"] {
        let id = c.id(a);
        let has = c.code(&id).await.map(|(h, _)| h).ok();
        if has.as_deref() == Some(agent_hash.as_str()) {
            println!("  skip {id} (deployed)");
        } else {
            let t = c
                .deploy(
                    &format!("deploy {id}"),
                    &c.acct(a)?,
                    agent_wasm.clone(),
                    json!({"owner_id": c.id("alice"), "base_address": base[a]}),
                )
                .await?;
            out.tx(&t);
            ok(&t)?;
        }
        let st = c.view(&id, "get_status", json!({})).await?;
        let w = c.view(&id, "get_base_wallet", json!({})).await?;
        let checks = json!({
            "token": st["token"] == json!(c.id("xusd")),
            "admin": st["admin"] == json!(c.cfg.root),
            "mpc": w["mpc"] == json!(MPC_SIGNER),
            "base_address": w["address"] == json!(base[a]),
        });
        out.push("agent_checks", json!({"agent": id, "checks": checks}));
        if checks
            .as_object()
            .is_some_and(|m| m.values().any(|v| v != true))
        {
            out.write()?;
            return Err(format!(
                "{id}: {checks}. Build with make build-testnet (AGENT_EXIT_TOKEN={}, AGENT_EXIT_ADMIN={})",
                c.id("xusd"),
                c.cfg.root
            ));
        }
    }
    out.write()?;

    println!("runner-keys");
    let alice = c.acct("alice")?;
    for a in ["translator", "cheat"] {
        let id = c.id(a);
        let name = format!("{a}-alice-runner");
        let fresh = c.keys.get(&name).is_none();
        let sk = c.keys.get_or_create(&name)?;
        let st = c.view(&id, "get_status", json!({})).await?;
        let pk = sk.public_key().to_string();
        if !fresh
            && st["known_keys"]
                .as_array()
                .is_some_and(|k| k.contains(&json!(pk)))
        {
            println!("  skip {id} runner key (set)");
            continue;
        }
        let t = c
            .call(
                &format!("{a}: set_runner_key"),
                &alice,
                &id,
                "set_runner_key",
                json!({"public_key": pk}),
                1,
                100,
            )
            .await;
        out.tx(&t);
        ok(&t)?;
        if t.events("runner_key_confirmed").is_empty() {
            return Err(format!("{id}: runner key not confirmed: {:?}", t.logs));
        }
    }

    println!("lock-translator");
    let tr = c.acct("translator")?;
    if c.keys(tr.id.as_ref())
        .await?
        .iter()
        .any(|k| k.public_key == tr.pk)
    {
        let t = c
            .delete_key("translator: delete setup key", &tr, &tr.pk.clone())
            .await?;
        out.tx(&t);
        ok(&t)?;
    }
    let keys = c.keys(tr.id.as_ref()).await?;
    out.set("translator_keys_after_lock", json!(keys));
    if keys.iter().any(|k| k.is_full()) {
        return Err("translator still has a full-access key".into());
    }
    out.write()?;

    println!("seed-world");
    let xusd_id = c.id("xusd");
    let pay = |label: String, from: &str, to: &str, amount: u128, msg: String| {
        let from = c.acct(from);
        let to = c.id(to);
        let xusd_id = xusd_id.clone();
        async move {
            let from = from?;
            Ok::<Tx, String>(
                c.call(
                    &label,
                    &from,
                    &xusd_id,
                    "ft_transfer_call",
                    json!({"receiver_id": to, "amount": amount.to_string(), "msg": msg}),
                    1,
                    150,
                )
                .await,
            )
        }
    };
    let e = c
        .view(&c.id("translator"), "get_earnings", json!({}))
        .await?;
    if e["gross"].as_u64().unwrap_or(0) < (500 * USD) as u64 {
        for p in ["p1", "p2", "p3", "p4", "p5", "p6"] {
            for j in 0..5 {
                let t = pay(
                    format!("{p} pays translator 12.00"),
                    p,
                    "translator",
                    12 * USD,
                    json!({"kind": "pay", "job_id": format!("seed-{p}-{j}")}).to_string(),
                )
                .await?;
                out.push("seed", t.json());
                ok(&t)?;
            }
        }
        for (who, a) in [("alice", 50), ("alice", 50), ("x.alice", 40)] {
            let t = pay(
                format!("{who} pays translator {a}.00"),
                who,
                "translator",
                a * USD,
                String::new(),
            )
            .await?;
            out.push("seed", t.json());
            ok(&t)?;
        }
    }
    let e = c.view(&c.id("cheat"), "get_earnings", json!({})).await?;
    if e["gross"].as_u64().unwrap_or(0) < (500 * USD) as u64 {
        for (who, a) in [
            ("alice", 100),
            ("alice", 100),
            ("alice", 100),
            ("x.alice", 100),
            ("x.alice", 100),
        ] {
            let t = pay(
                format!("{who} pays cheat {a}.00"),
                who,
                "cheat",
                a * USD,
                String::new(),
            )
            .await?;
            out.push("seed", t.json());
            ok(&t)?;
        }
    }
    out.set(
        "earnings",
        json!({
            "translator": c.view(&c.id("translator"), "get_earnings", json!({})).await?,
            "cheat": c.view(&c.id("cheat"), "get_earnings", json!({})).await?,
        }),
    );
    out.set("pass", json!(true));
    out.write()
}

// ------------------------------------------------------------------ G1

async fn g1(c: &Chain, secrets: Vec<String>) -> Res<()> {
    let mut out = Out::new("out/gates/g1.json", secrets);
    out.set("schema", json!("gate-g1.v1"));
    out.set("gate", json!("G1 key handover on a real node: the old runner key and the deleted setup key fail with the node's own error"));
    let agent_wasm = wasm("AGENT_WASM", "agent_exit.wasm")?;
    let stamp = now_ns() / 1_000_000_000;
    let name = format!("g1-{stamp}");
    let id = c.id(&name);
    println!("g1-deploy {id}");
    let t = c
        .create(&format!("create {id}"), &c.root, &id, &name, 7)
        .await?;
    out.tx(&t);
    ok(&t)?;
    let agent = c.acct(&name)?;
    let t = c
        .deploy(
            "deploy + new(owner alice)",
            &agent,
            agent_wasm.clone(),
            json!({"owner_id": c.id("alice")}),
        )
        .await?;
    out.tx(&t);
    ok(&t)?;
    let alice = c.acct("alice")?;
    let alice_runner = c.keys.get_or_create(&format!("{name}-alice-runner"))?;
    let t = c
        .call(
            "alice: set_runner_key",
            &alice,
            &id,
            "set_runner_key",
            json!({"public_key": alice_runner.public_key().to_string()}),
            1,
            100,
        )
        .await;
    out.tx(&t);
    ok(&t)?;
    let t = c
        .call(
            "register agent on xusd",
            &c.root,
            &c.id("xusd"),
            "mint",
            json!({"account_id": id, "amount": "0"}),
            0,
            30,
        )
        .await;
    out.tx(&t);
    ok(&t)?;
    let setup_pk = agent.pk.clone();
    let t = c
        .delete_key("delete the setup full-access key", &agent, &setup_pk)
        .await?;
    out.tx(&t);
    ok(&t)?;
    let t = c
        .call(
            "alice lists at 10.00",
            &alice,
            &id,
            "list_for_sale",
            json!({"price": (10 * USD).to_string(), "code_hash": sha256_b58(&agent_wasm)}),
            1,
            50,
        )
        .await;
    out.tx(&t);
    ok(&t)?;

    println!("g1-sell");
    let before = c.keys(&id).await?;
    let bob = c.acct("bob")?;
    let bob_runner = c.keys.get_or_create(&format!("{name}-bob-runner"))?;
    let st = c.view(&id, "get_status", json!({})).await?;
    let msg = json!({"kind": "buy", "listing_version": st["listing_version"],
                     "known_keys_hash": st["known_keys_hash"], "buyer_runner_pk": bob_runner.public_key().to_string()});
    let t = c
        .call(
            "bob buys (one ft_transfer_call)",
            &bob,
            &c.id("xusd"),
            "ft_transfer_call",
            json!({"receiver_id": id, "amount": (10 * USD).to_string(), "msg": msg.to_string()}),
            1,
            300,
        )
        .await;
    out.tx(&t);
    ok(&t)?;
    let after = c.keys(&id).await?;
    out.set("keys_before", json!(before));
    out.set("keys_after", json!(after));

    println!("g1-old-keys");
    let old = acct(&id, &alice_runner)?;
    let t_old = c
        .call(
            "alice's old runner key: heartbeat",
            &old,
            &id,
            "heartbeat",
            json!({}),
            0,
            30,
        )
        .await;
    let setup = acct(&id, &c.keys.get(&name).ok_or("setup key")?)?;
    let t_setup = c
        .call(
            "deleted setup key: heartbeat",
            &setup,
            &id,
            "heartbeat",
            json!({}),
            0,
            30,
        )
        .await;
    let new = acct(&id, &bob_runner)?;
    let t_new = c
        .call(
            "bob's runner key: heartbeat",
            &new,
            &id,
            "heartbeat",
            json!({}),
            0,
            30,
        )
        .await;
    for t in [&t_old, &t_setup, &t_new] {
        out.tx(t);
    }
    let pass = before.len() == 1
        && before[0].public_key == alice_runner.public_key().to_string()
        && after.len() == 1
        && after[0].public_key == bob_runner.public_key().to_string()
        && !t_old.ok
        && is_missing_key(t_old.err())
        && !t_setup.ok
        && is_missing_key(t_setup.err())
        && t_new.ok;
    out.set("old_runner_key_error", json!(t_old.error));
    out.set("setup_key_error", json!(t_setup.error));
    out.set("pass", json!(pass));
    out.set(
        "result",
        json!(if pass {
            "G1 PASS: prototype A (locked account) works on testnet"
        } else {
            "G1 FAIL: see txs; fallback is prototype B (keyed owner)"
        }),
    );
    out.write()?;
    if pass {
        Ok(())
    } else {
        Err("G1 failed (see out/gates/g1.json)".into())
    }
}

// ------------------------------------------------------------------ G2

async fn g2(c: &Chain, env: &BTreeMap<String, String>, secrets: Vec<String>) -> Res<()> {
    let mut out = Out::new("out/gates/g2-sign.json", secrets);
    out.set("schema", json!("gate-g2-sign.v1"));
    let agent = c.id("translator");
    out.set("agent_account", json!(agent));
    println!("g2-derive");
    let mpc_pk = c.view(MPC_SIGNER, "public_key", json!({})).await?;
    let mpc_pk = mpc_pk
        .as_str()
        .ok_or("MPC public_key is not a string")?
        .to_string();
    let derived = base_wallet::derive_address(
        &base_wallet::parse_near_secp_pk(&mpc_pk)?,
        &agent,
        agent_core::base::BASE_PATH,
    );
    let w = c.view(&agent, "get_base_wallet", json!({})).await?;
    out.set("mpc", json!({"signer": MPC_SIGNER, "public_key": mpc_pk}));
    out.set("derived_address", json!(derived));
    out.set("contract_address", w["address"].clone());
    out.set("domain_separator", w["domain_separator"].clone());
    if w["address"] != json!(derived) {
        out.set("pass", json!(false));
        out.write()?;
        return Err(format!("derived {derived} != contract {}", w["address"]));
    }

    println!("g2-sign");
    let payee = match (
        &c.cfg.base_payee,
        env.get("BASE_RELAYER_KEY").filter(|k| !k.is_empty()),
    ) {
        (Some(p), _) => p.clone(),
        (None, Some(k)) => relayer_address(k)?,
        (None, None) => return Err("set BASE_PAYEE or BASE_RELAYER_KEY".into()),
    };
    out.set("payee", json!(payee));
    let owner = c.view(&agent, "get_status", json!({})).await?["owner"]
        .as_str()
        .unwrap_or_default()
        .to_string();
    let (owner_name, runner_key) = if owner == c.id("bob") {
        ("bob", "translator-bob-runner")
    } else {
        ("alice", "translator-alice-runner")
    };
    let t = c
        .call(
            &format!("{owner_name}: set_limits with Base (cap 5.00 USDC)"),
            &c.acct(owner_name)?,
            &agent,
            "set_limits",
            demo_limits(&c.id("summarizer"), Some((5 * USD, &payee))),
            1,
            50,
        )
        .await;
    out.tx(&t);
    ok(&t)?;
    let runner = c.runner("translator", runner_key)?;
    let mut samples = vec![];
    for i in 0..5 {
        let t = c
            .call(
                &format!("pay_base sample {}", i + 1),
                &runner,
                &agent,
                "pay_base",
                json!({"to": payee, "amount": "10000", "valid_secs": 300}),
                0,
                300,
            )
            .await;
        let signed = t.events("base_auth_signed");
        let check = signed.first().map(base_wallet::check_auth);
        samples.push(json!({
            "ms": t.ms, "ok": t.ok && !signed.is_empty(), "error": t.error,
            "sign_failed": t.events("sign_failed"), "auth": signed.first(),
            "offline_check": check.as_ref().map(|r| match r { Ok(a) => json!({"signer": a}), Err(e) => json!({"error": e}) }),
            "tx": t.hash, "explorer": t.hash.as_ref().map(|h| format!("{EXPLORER_TX}{h}")),
        }));
    }
    let good: Vec<&Value> = samples
        .iter()
        .filter(|s| s["ok"] == true && s["offline_check"]["signer"] == json!(derived))
        .collect();
    let mut ms: Vec<u64> = good.iter().filter_map(|s| s["ms"].as_u64()).collect();
    ms.sort();
    out.set(
        "latency_ms",
        json!({"min": ms.first(), "median": ms.get(ms.len() / 2), "max": ms.last(), "n": ms.len()}),
    );
    out.set("sign_args", json!({"method": "sign", "args": {"request": {"path": "wallet-1", "payload_v2": {"Ecdsa": "<digest hex>"}, "domain_id": 0}}, "deposit_yocto": 1}));
    out.set("samples", json!(samples));
    let signed_ok = !good.is_empty();
    out.set("pass_sign", json!(signed_ok));
    out.write()?;
    if !signed_ok {
        return Err(
            "no MPC signature recovered to the wallet address (see out/gates/g2-sign.json)".into(),
        );
    }

    println!("g2-relay");
    let relay = format!("{ROOT_DIR}/scripts/testnet/g2-relay.mjs");
    if !std::path::Path::new(&format!("{ROOT_DIR}/scripts/testnet/node_modules")).exists() {
        return Err("signing passed; relay skipped: run `make testnet-setup` (npm install in scripts/testnet), then `make gate-g2-relay`".into());
    }
    let key = env
        .get("BASE_RELAYER_KEY")
        .filter(|k| !k.is_empty())
        .ok_or("BASE_RELAYER_KEY missing (relay skipped)")?;
    let status = tokio::process::Command::new("node")
        .arg(relay)
        .arg(format!("{ROOT_DIR}/out/gates/g2-sign.json"))
        .env_clear()
        .env("PATH", std::env::var("PATH").unwrap_or_default())
        .env("HOME", std::env::var("HOME").unwrap_or_default())
        .env("BASE_RELAYER_KEY", key)
        .env("BASE_SEPOLIA_RPC", &c.cfg.base_rpc)
        .status()
        .await
        .map_err(|e| format!("node: {e}"))?;
    if status.success() {
        Ok(())
    } else {
        Err("g2-relay.mjs failed (see out/gates/g2.json)".into())
    }
}

/// Ethereum address of the relayer key (`0x` + 64 hex).
fn relayer_address(key: &str) -> Res<String> {
    let b = hex::decode(key.trim().trim_start_matches("0x"))
        .map_err(|_| "BASE_RELAYER_KEY is not hex")?;
    let sk =
        k256::SecretKey::from_slice(&b).map_err(|_| "BASE_RELAYER_KEY is not a secp256k1 key")?;
    Ok(base_wallet::address(&sk.public_key()))
}

// ------------------------------------------------------------------ G6

async fn g6(c: &Chain, env: &BTreeMap<String, String>, secrets: Vec<String>) -> Res<()> {
    let mut out = Out::new("out/gates/g6.json", secrets);
    out.set("schema", json!("gate-g6.v1"));
    let key = env
        .get("NEARAI_API_KEY")
        .filter(|k| !k.is_empty())
        .ok_or("NEARAI_API_KEY missing")?;
    let agent = c.id("translator");
    let payer = c.id("p1");
    let amount = USD;
    let job = format!("g6-{}", now_ns() / 1_000_000_000);
    let http = reqwest::Client::builder()
        .timeout(Duration::from_secs(60))
        .redirect(reqwest::redirect::Policy::none())
        .build()
        .map_err(|e| e.to_string())?;
    let mut models = vec![c.cfg.nearai_model.clone()];
    models.extend(c.cfg.nearai_model_2.clone());
    let mut proofs = vec![];
    for model in &models {
        println!("g6-completion ({model})");
        // The exact bytes sent: the contract hashes this string.
        let request = json!({
            "model": model,
            "messages": [{"role": "user", "content": "Translate to French: the agent was sold."}],
            "max_tokens": 32,
            "metadata": {"agent": agent, "job_id": job, "payer": payer, "amount": amount.to_string()},
        })
        .to_string();
        let t0 = Instant::now();
        let r = http
            .post(format!("{}/v1/chat/completions", c.cfg.nearai_base))
            .bearer_auth(key)
            .header("content-type", "application/json")
            .body(request.clone())
            .send()
            .await
            .map_err(|e| format!("completion: {e}"))?;
        let status = r.status().as_u16();
        let body = r.bytes().await.map_err(|e| e.to_string())?;
        let resp: Value = serde_json::from_slice(&body).unwrap_or(Value::Null);
        if status != 200 {
            out.push(
                "samples",
                json!({"model": model, "status": status, "error": resp}),
            );
            continue;
        }
        let chat_id = resp["id"].as_str().unwrap_or_default().to_string();
        println!("g6-signature");
        let s = http
            .get(format!("{}/v1/signature/{chat_id}", c.cfg.nearai_base))
            .query(&[("model", model.as_str()), ("signing_algo", "ecdsa")])
            .bearer_auth(key)
            .send()
            .await
            .map_err(|e| format!("signature: {e}"))?;
        let sig: Value = s.json().await.unwrap_or(Value::Null);
        use sha2::Digest;
        let req_sha = hex::encode(sha2::Sha256::digest(request.as_bytes()));
        let resp_sha = hex::encode(sha2::Sha256::digest(&body));
        let text = sig["text"].as_str().unwrap_or_default();
        let form = if text == format!("{model}:{req_sha}:{resp_sha}") {
            "provider (model:req:resp), the form the contract checks"
        } else if text == format!("{req_sha}:{resp_sha}") {
            "gateway (req:resp): the contract expects model:req:resp, adjust verified.rs"
        } else {
            "unknown: the signed text does not match our hashes"
        };
        let recovered = eip191_recover(text, sig["signature"].as_str().unwrap_or_default());
        proofs.push(json!({"model": model, "request": request, "response_sha256": resp_sha,
                           "signature": sig["signature"], "signing_address": sig["signing_address"]}));
        out.push("samples", json!({
            "model": model, "chat_id": chat_id, "ms": t0.elapsed().as_millis(),
            "signed_text": text, "text_form": form, "request_sha256": req_sha, "response_sha256": resp_sha,
            "signing_address": sig["signing_address"], "signing_algo": sig["signing_algo"],
            "recovered_offline": recovered, "request_len": request.len(),
        }));
    }
    out.write()?;
    let Some(p) = proofs.first().cloned() else {
        return Err("no NEAR AI signature (see out/gates/g6.json)".into());
    };

    println!("g6-on-chain");
    let signer = p["signing_address"]
        .as_str()
        .unwrap_or_default()
        .to_string();
    let t = c
        .call(
            "registry admin: add_signer",
            &c.root,
            &agent,
            "add_signer",
            json!({"address": signer}),
            0,
            30,
        )
        .await;
    out.tx(&t);
    ok(&t)?;
    let msg = json!({"kind": "pay", "job_id": job, "nearai": {
        "model": p["model"], "request": p["request"], "response_sha256": p["response_sha256"],
        "signature": p["signature"].as_str().unwrap_or_default().trim_start_matches("0x"),
    }});
    let t = c
        .call(
            "p1 pays 1.00 with the NEAR AI proof",
            &c.acct("p1")?,
            &c.id("xusd"),
            "ft_transfer_call",
            json!({"receiver_id": agent, "amount": amount.to_string(), "msg": msg.to_string()}),
            1,
            150,
        )
        .await;
    out.tx(&t);
    let payment = t.events("payment").first().cloned().unwrap_or(Value::Null);
    let pass = payment["verified"] == true;
    out.set("payment_event", payment);
    out.set(
        "signer_stability",
        json!(proofs
            .iter()
            .map(|p| json!({"model": p["model"], "signing_address": p["signing_address"]}))
            .collect::<Vec<_>>()),
    );
    out.set("pass", json!(pass));
    out.write()?;
    if pass {
        Ok(())
    } else {
        Err("the contract did not mark the payment verified (see out/gates/g6.json)".into())
    }
}

/// EIP-191 personal_sign recovery of `0x` + 65-byte hex.
fn eip191_recover(text: &str, sig_hex: &str) -> Option<String> {
    use k256::ecdsa::{RecoveryId, Signature, VerifyingKey};
    let sig = hex::decode(sig_hex.trim_start_matches("0x")).ok()?;
    if sig.len() != 65 {
        return None;
    }
    let mut m = format!("\x19Ethereum Signed Message:\n{}", text.len()).into_bytes();
    m.extend_from_slice(text.as_bytes());
    let h = agent_core::base::keccak(&m);
    let s = Signature::from_slice(&sig[..64]).ok()?;
    let v = match sig[64] {
        0 | 27 => 0,
        1 | 28 => 1,
        _ => return None,
    };
    let vk = VerifyingKey::recover_from_prehash(&h, &s, RecoveryId::from_byte(v)?).ok()?;
    Some(base_wallet::address(&k256::PublicKey::from(&vk)))
}

// ------------------------------------------------------------------ scenes

/// scan-input.v1 from the chain: code, methods and keys from both RPCs.
async fn scan_agent(
    c: &Chain,
    id: &str,
    quoted_version: Option<u64>,
) -> Res<(Value, scan::Report)> {
    let (code_hash, code) = c.code(id).await?;
    let methods = crate::wasm::exports(&code)?;
    let keys = c.keys_on(&c.net, id).await?;
    let keys2 = c.keys_on(&c.net2, id).await?;
    let st = c.view(id, "get_status", json!({})).await?;
    let w = c.view(id, "get_base_wallet", json!({})).await?;
    let listing = c.view(id, "get_listing", json!({})).await?;
    let expected_hash = sha256_b58(&wasm("AGENT_WASM", "agent_exit.wasm")?);
    let version = quoted_version.unwrap_or(st["listing_version"].as_u64().unwrap_or(0));
    let input = json!({
        "schema": "scan-input.v1",
        "agent": id,
        "now_ns": now_ns(),
        "expected": {"code_hash": expected_hash, "listing_version": version,
                     "admin": c.cfg.root, "token": c.id("xusd"),
                     "price": listing["price"]},
        "chain": {"code_hash": code_hash, "methods": methods, "keys": keys, "keys_second_provider": keys2, "subaccounts": []},
        "contract": {
            "status": st["status"], "owner": st["owner"], "listing_version": st["listing_version"],
            "known_keys": st["known_keys"], "known_keys_hash": st["known_keys_hash"],
            "open_approvals": st["open_approvals"],
            "open_auth_until": w["open_auth_until"].as_str().and_then(|s| s.parse::<u64>().ok()).unwrap_or(0),
            "frozen": st["frozen"], "admin": st["admin"], "token": st["token"],
            "paid_to_total": st["paid_to_total"],
        },
        "listing": if listing.is_null() { Value::Null } else {
            json!({"listing_version": listing["listing_version"], "price": listing["price"], "terms": listing["terms"]})
        },
    });
    let parsed: scan::ScanInput =
        serde_json::from_value(input.clone()).map_err(|e| format!("scan input: {e}"))?;
    Ok((input, scan::scan(&parsed)))
}

async fn wait_no_open_auth(c: &Chain, id: &str) -> Res<()> {
    loop {
        let w = c.view(id, "get_base_wallet", json!({})).await?;
        let until: u64 = w["open_auth_until"]
            .as_str()
            .and_then(|s| s.parse().ok())
            .unwrap_or(0);
        let now = now_ns();
        if until <= now {
            return Ok(());
        }
        let secs = (until - now) / 1_000_000_000 + 2;
        println!(
            "  a Base authorization is open for {secs} s more (A9: no listing until then); waiting"
        );
        tokio::time::sleep(Duration::from_secs(secs.min(60))).await;
    }
}

async fn list(c: &Chain, out: &mut Out, owner: &str, agent: &str, price: u128) -> Res<()> {
    let id = c.id(agent);
    let st = c.view(&id, "get_status", json!({})).await?;
    if st["status"] == "Listed" {
        return Ok(());
    }
    wait_no_open_auth(c, &id).await?;
    let (hash, _) = c.code(&id).await?;
    let t = c
        .call(
            &format!("{owner} lists {agent} at {}.00", price / USD),
            &c.acct(owner)?,
            &id,
            "list_for_sale",
            json!({"price": price.to_string(), "code_hash": hash}),
            1,
            50,
        )
        .await;
    out.tx(&t);
    ok(&t)
}

async fn buy(c: &Chain, out: &mut Out, buyer: &str, agent: &str, price: u128) -> Res<(Tx, String)> {
    let id = c.id(agent);
    let (input, report) = scan_agent(c, &id, None).await?;
    out.set("scan_input", input);
    out.set("scan", serde_json::to_value(&report).unwrap_or_default());
    if report.blocks() {
        return Err(format!(
            "{buyer}'s scan of {agent}: {} {:?}",
            report.verdict,
            report.codes()
        ));
    }
    let key_name = format!("{agent}-{buyer}-runner");
    let sk = c.keys.get_or_create(&key_name)?;
    let st = c.view(&id, "get_status", json!({})).await?;
    let msg = json!({"kind": "buy", "listing_version": st["listing_version"], "known_keys_hash": st["known_keys_hash"],
                     "buyer_runner_pk": sk.public_key().to_string()});
    let t = c
        .call(
            &format!("{buyer} buys {agent} (one ft_transfer_call)"),
            &c.acct(buyer)?,
            &c.id("xusd"),
            "ft_transfer_call",
            json!({"receiver_id": id, "amount": price.to_string(), "msg": msg.to_string()}),
            1,
            300,
        )
        .await;
    out.tx(&t);
    ok(&t)?;
    Ok((t, key_name))
}

async fn balance(c: &Chain, who: &str) -> u128 {
    c.view(
        &c.id("xusd"),
        "ft_balance_of",
        json!({"account_id": c.id(who)}),
    )
    .await
    .ok()
    .and_then(|v| v.as_str().and_then(|s| s.parse().ok()))
    .unwrap_or(0)
}

async fn scene(c: &Chain, n: u8, secrets: Vec<String>) -> Res<()> {
    let mut out = Out::new(&format!("out/scenes/scene-{n}.json"), secrets);
    out.set("schema", json!("scene.v1"));
    out.set("scene", json!(n));
    out.set(
        "what",
        json!(info(scene_step(n)).what.replace("ROOT", &c.cfg.root)),
    );
    println!("scene-{n}");
    let tr = c.id("translator");
    let r = match n {
        3 => {
            out.set("translator", c.view(&tr, "get_earnings", json!({})).await?);
            out.set(
                "cheat",
                c.view(&c.id("cheat"), "get_earnings", json!({})).await?,
            );
            Ok(())
        }
        4 => {
            list(c, &mut out, "alice", "cheat", 500 * USD).await?;
            let (input, report) = scan_agent(c, &c.id("cheat"), None).await?;
            out.set("scan_input", input);
            out.set("scan", serde_json::to_value(&report).unwrap_or_default());
            out.set("buy_sent", json!(false));
            if report.blocks() && report.codes().contains(&"full_access_key_present") {
                Ok(())
            } else {
                Err(format!(
                    "expected a block for full_access_key_present, got {} {:?}",
                    report.verdict,
                    report.codes()
                ))
            }
        }
        5 => {
            list(c, &mut out, "alice", "translator", 1_000 * USD).await?;
            let before = c.keys(&tr).await?;
            let alice0 = balance(c, "alice").await;
            let (t, _) = buy(c, &mut out, "bob", "translator", 1_000 * USD).await?;
            let after = c.keys(&tr).await?;
            let st = c.view(&tr, "get_status", json!({})).await?;
            out.set("keys_before", json!(before));
            out.set("keys_after", json!(after));
            out.set("sale", json!(t.events("sale")));
            out.set(
                "alice_received",
                json!((balance(c, "alice").await.saturating_sub(alice0)).to_string()),
            );
            out.set("status_after", st.clone());
            if st["owner"] == json!(c.id("bob")) {
                Ok(())
            } else {
                Err("bob is not the owner".into())
            }
        }
        1 => {
            let st = c.view(&tr, "get_status", json!({})).await?;
            if st["owner"] != json!(c.id("bob")) {
                return Err("run scene-5 first (the sale comes before the break-in)".into());
            }
            let alice = c.acct("alice")?;
            let new_key = c.keys.get_or_create("alice-break-in-key")?;
            let tries = [
                (
                    "withdraw 1.00",
                    "withdraw",
                    json!({"amount": USD.to_string()}),
                    1,
                    50,
                ),
                (
                    "set_runner_key (her own key)",
                    "set_runner_key",
                    json!({"public_key": new_key.public_key().to_string()}),
                    1,
                    100,
                ),
                (
                    "list_for_sale 1.00",
                    "list_for_sale",
                    json!({"price": USD.to_string(), "code_hash": "x"}),
                    1,
                    50,
                ),
                (
                    "set_limits",
                    "set_limits",
                    demo_limits(&c.id("alice"), None),
                    1,
                    50,
                ),
                (
                    "nft_transfer the deed to herself",
                    "nft_transfer",
                    json!({"receiver_id": c.id("alice"), "token_id": "deed"}),
                    1,
                    50,
                ),
                (
                    "pay_base (the Base wallet)",
                    "pay_base",
                    json!({"to": "0x00000000000000000000000000000000000000a1", "amount": "1000000"}),
                    0,
                    50,
                ),
            ];
            let mut all_refused = true;
            for (label, method, args, yocto, tgas) in tries {
                let t = c
                    .call(
                        &format!("alice: {label}"),
                        &alice,
                        &tr,
                        method,
                        args,
                        yocto,
                        tgas,
                    )
                    .await;
                all_refused &= !t.ok;
                out.tx(&t);
            }
            let old = c.runner("translator", "translator-alice-runner")?;
            let t_old = c
                .call(
                    "alice's old runner key: heartbeat",
                    &old,
                    &tr,
                    "heartbeat",
                    json!({}),
                    0,
                    30,
                )
                .await;
            let setup = c.acct("translator")?;
            let t_setup = c
                .call(
                    "translator's deleted setup key: heartbeat",
                    &setup,
                    &tr,
                    "heartbeat",
                    json!({}),
                    0,
                    30,
                )
                .await;
            let bob_runner = c.runner("translator", "translator-bob-runner")?;
            let t_bob = c
                .call(
                    "bob's runner key: heartbeat",
                    &bob_runner,
                    &tr,
                    "heartbeat",
                    json!({}),
                    0,
                    30,
                )
                .await;
            for t in [&t_old, &t_setup, &t_bob] {
                out.tx(t);
            }
            out.set("keys", json!(c.keys(&tr).await?));
            if all_refused
                && is_missing_key(t_old.err())
                && is_missing_key(t_setup.err())
                && t_bob.ok
            {
                Ok(())
            } else {
                Err("a break-in attempt did not fail as expected (see the scene file)".into())
            }
        }
        2 => {
            let bob = c.acct("bob")?;
            let t = c
                .call(
                    "bob: set_limits (cap 50.00, payee summarizer, split 80/20)",
                    &bob,
                    &tr,
                    "set_limits",
                    demo_limits(
                        &c.id("summarizer"),
                        c.cfg.base_payee.as_deref().map(|p| (5 * USD, p)),
                    ),
                    1,
                    50,
                )
                .await;
            out.tx(&t);
            ok(&t)?;
            for (p, job) in [("p1", "scene2-a"), ("p2", "scene2-b")] {
                let t = c.call(&format!("{p} pays 25.00 for a job"), &c.acct(p)?, &c.id("xusd"), "ft_transfer_call",
                               json!({"receiver_id": tr, "amount": (25 * USD).to_string(),
                                      "msg": json!({"kind": "pay", "job_id": format!("{job}-{}", now_ns())}).to_string()}), 1, 150).await;
                out.tx(&t);
                ok(&t)?;
            }
            let runner = c.runner("translator", "translator-bob-runner")?;
            let t8 = c
                .call(
                    "runner pays summarizer 8.00",
                    &runner,
                    &tr,
                    "pay",
                    json!({"payee": c.id("summarizer"), "amount": (8 * USD).to_string()}),
                    0,
                    100,
                )
                .await;
            out.tx(&t8);
            let t45 = c
                .call(
                    "runner pays summarizer 45.00",
                    &runner,
                    &tr,
                    "pay",
                    json!({"payee": c.id("summarizer"), "amount": (45 * USD).to_string()}),
                    0,
                    100,
                )
                .await;
            out.tx(&t45);
            out.set("limits_after", c.view(&tr, "get_limits", json!({})).await?);
            if t8.ok && !t45.ok && t45.err().contains("PolicyViolation:DailyCap") {
                Ok(())
            } else {
                Err("expected 8.00 ok and 45.00 refused DailyCap".into())
            }
        }
        _ => {
            list(c, &mut out, "bob", "translator", 1_200 * USD).await?;
            let (_, key) = buy(c, &mut out, "summarizer", "translator", 1_200 * USD).await?;
            let st = c.view(&tr, "get_status", json!({})).await?;
            let keys = c.keys(&tr).await?;
            let summ_pk = c
                .keys
                .get(&key)
                .map(|k| k.public_key().to_string())
                .unwrap_or_default();
            let locked_out = st["owner"] == json!(c.id("summarizer"))
                && keys.len() == 1
                && keys[0].public_key == summ_pk
                && !keys.iter().any(|k| k.is_full())
                && st["incoming_keys"].as_array().is_some_and(|a| a.is_empty())
                && st["retiring_keys"].as_array().is_some_and(|a| a.is_empty());
            out.set("verify_lockout", json!({"account": c.id("bob"), "locked_out": locked_out, "keys": keys, "owner": st["owner"]}));
            out.set(
                "note",
                json!("scripted client; MCP testnet writes are not wired yet"),
            );
            if locked_out {
                Ok(())
            } else {
                Err("bob is not locked out".into())
            }
        }
    };
    out.set("pass", json!(r.is_ok()));
    if let Err(e) = &r {
        out.set("error", json!(e));
    }
    out.write()?;
    r
}
