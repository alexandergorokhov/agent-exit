//! The testnet day, as data. `tools/testnet` (the live runner, its own
//! workspace because it pulls the NEAR RPC client) executes these steps;
//! this crate only loads the env file, refuses anything but testnet, and
//! describes the plan. `--dry-run` prints the plan from here: offline, no
//! secret is ever read into the output.
//!
//! Secrets live in `~/.agent-exit.env` (KEY=VALUE lines, outside the repo)
//! and in `~/.agent-exit/keys/` (keys this runner creates, mode 0600).

use serde_json::{json, Value};
use std::collections::BTreeMap;

pub const NETWORK: &str = "testnet";
pub const DEFAULT_ROOT: &str = "agentexit.testnet";
pub const MPC_SIGNER: &str = "v1.signer-prod.testnet";
pub const BASE_CHAIN_ID: u64 = 84_532;
pub const NEARAI_HOST: &str = "cloud-api.near.ai";
pub const EXPLORER_TX: &str = "https://testnet.nearblocks.io/txns/";
pub const BASE_EXPLORER_TX: &str = "https://sepolia.basescan.org/tx/";

/// NEAR RPC hosts the runner may talk to (same list as the MCP server).
pub const TESTNET_RPC_HOSTS: [&str; 5] = [
    "rpc.testnet.near.org",
    "archival-rpc.testnet.near.org",
    "test.rpc.fastnear.com",
    "rpc.testnet.fastnear.com",
    "archival-rpc.testnet.fastnear.com",
];
/// Base Sepolia RPC hosts (chain id 84532 is checked again before any send).
pub const BASE_SEPOLIA_RPC_HOSTS: [&str; 3] = [
    "sepolia.base.org",
    "base-sepolia-rpc.publicnode.com",
    "base-sepolia.drpc.org",
];

/// Variables whose values are secret: never printed, never written to out/.
pub const SECRET_VARS: [&str; 4] = [
    "AGENT_EXIT_ROOT_KEY",
    "NEARAI_API_KEY",
    "BASE_RELAYER_KEY",
    "ANTHROPIC_API_KEY",
];

/// Parse `KEY=VALUE` lines (`#` comments, optional `export `, optional
/// quotes). Later lines win.
pub fn parse_env(text: &str) -> BTreeMap<String, String> {
    let mut m = BTreeMap::new();
    for line in text.lines() {
        let l = line.trim();
        if l.is_empty() || l.starts_with('#') {
            continue;
        }
        let l = l.strip_prefix("export ").unwrap_or(l);
        if let Some((k, v)) = l.split_once('=') {
            let v = v.trim();
            let v = v
                .strip_prefix('"')
                .and_then(|x| x.strip_suffix('"'))
                .or_else(|| v.strip_prefix('\'').and_then(|x| x.strip_suffix('\'')))
                .unwrap_or(v);
            m.insert(k.trim().to_string(), v.to_string());
        }
    }
    m
}

/// `~/.agent-exit.env`, or `AGENT_EXIT_ENV` if set.
pub fn env_path() -> String {
    std::env::var("AGENT_EXIT_ENV").unwrap_or_else(|_| {
        format!(
            "{}/.agent-exit.env",
            std::env::var("HOME").unwrap_or_default()
        )
    })
}

fn host_of(url: &str) -> Option<String> {
    let rest = url.strip_prefix("https://")?;
    let host = rest.split(['/', '?', '#']).next()?;
    if host.contains('@') || host.contains(':') || host.is_empty() {
        return None;
    }
    Some(host.to_ascii_lowercase())
}

pub fn is_testnet_account(a: &str) -> bool {
    a.ends_with(".testnet")
        && a.len() <= 64
        && a.split('.').all(|p| {
            !p.is_empty()
                && p.bytes()
                    .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == b'-' || c == b'_')
        })
}

/// Everything the runner needs, after the testnet-only checks.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Config {
    pub root: String,
    pub rpc: Vec<String>,
    pub base_rpc: String,
    pub nearai_base: String,
    pub nearai_model: String,
    pub nearai_model_2: Option<String>,
    pub base_payee: Option<String>,
    pub keys_dir: String,
    /// Which secrets are present (names only).
    pub secrets_present: Vec<String>,
}

impl Config {
    pub fn account(&self, name: &str) -> String {
        format!("{name}.{}", self.root)
    }

    pub fn agent(&self) -> String {
        self.account("translator")
    }
    pub fn cheat(&self) -> String {
        self.account("cheat")
    }
    pub fn xusd(&self) -> String {
        self.account("xusd")
    }
    pub fn has(&self, secret: &str) -> bool {
        self.secrets_present.iter().any(|s| s == secret)
    }
}

/// Testnet-only checks. Refuses: a network other than testnet, any account
/// not ending in `.testnet`, any RPC that is not https on a known testnet
/// host (or names mainnet), a Base RPC outside the Base Sepolia list.
pub fn check(env: &BTreeMap<String, String>) -> Result<Config, Vec<String>> {
    let mut errs = vec![];
    let get = |k: &str| env.get(k).map(String::as_str).filter(|v| !v.is_empty());
    match get("AGENT_EXIT_NETWORK") {
        Some(NETWORK) => {}
        Some(n) => errs.push(format!(
            "AGENT_EXIT_NETWORK={n}: only testnet is allowed, never mainnet"
        )),
        None => errs.push("AGENT_EXIT_NETWORK=testnet missing".into()),
    }
    let root = get("AGENT_EXIT_ROOT").unwrap_or(DEFAULT_ROOT).to_string();
    if !is_testnet_account(&root) {
        errs.push(format!("AGENT_EXIT_ROOT={root}: a .testnet account only"));
    }
    let rpc: Vec<String> = [
        get("AGENT_EXIT_RPC").unwrap_or("https://rpc.testnet.near.org"),
        get("AGENT_EXIT_RPC_2").unwrap_or("https://test.rpc.fastnear.com"),
    ]
    .iter()
    .map(|s| s.to_string())
    .collect();
    for u in &rpc {
        let ok = host_of(u).is_some_and(|h| TESTNET_RPC_HOSTS.contains(&h.as_str()))
            && !u.to_ascii_lowercase().contains("mainnet");
        if !ok {
            errs.push(format!(
                "refusing NEAR RPC {u}: https on {} only",
                TESTNET_RPC_HOSTS.join(", ")
            ));
        }
    }
    let base_rpc = get("BASE_SEPOLIA_RPC")
        .unwrap_or("https://sepolia.base.org")
        .to_string();
    if !host_of(&base_rpc).is_some_and(|h| BASE_SEPOLIA_RPC_HOSTS.contains(&h.as_str())) {
        errs.push(format!(
            "refusing Base RPC {base_rpc}: https on {} only",
            BASE_SEPOLIA_RPC_HOSTS.join(", ")
        ));
    }
    let nearai_base = get("NEARAI_BASE_URL")
        .unwrap_or("https://cloud-api.near.ai")
        .trim_end_matches('/')
        .to_string();
    if host_of(&nearai_base).as_deref() != Some(NEARAI_HOST) {
        errs.push(format!(
            "refusing NEARAI_BASE_URL {nearai_base}: https://{NEARAI_HOST} only"
        ));
    }
    let base_payee = get("BASE_PAYEE").map(|s| s.to_ascii_lowercase());
    if let Some(p) = &base_payee {
        let ok =
            p.len() == 42 && p.starts_with("0x") && p[2..].bytes().all(|c| c.is_ascii_hexdigit());
        if !ok {
            errs.push("BASE_PAYEE: 0x + 40 hex".into());
        }
    }
    let secrets_present = SECRET_VARS
        .iter()
        .filter(|k| get(k).is_some())
        .map(|k| k.to_string())
        .collect();
    if !errs.is_empty() {
        return Err(errs);
    }
    Ok(Config {
        root,
        rpc,
        base_rpc,
        nearai_base,
        nearai_model: get("NEARAI_MODEL")
            .unwrap_or("openai/gpt-oss-120b")
            .to_string(),
        nearai_model_2: get("NEARAI_MODEL_2").map(str::to_string),
        base_payee,
        keys_dir: get("AGENT_EXIT_KEYS_DIR")
            .map(str::to_string)
            .unwrap_or_else(|| {
                format!(
                    "{}/.agent-exit/keys",
                    std::env::var("HOME").unwrap_or_default()
                )
            }),
        secrets_present,
    })
}

/// The config a dry run shows when no env file exists: the defaults.
pub fn default_env() -> BTreeMap<String, String> {
    [("AGENT_EXIT_NETWORK", NETWORK)]
        .iter()
        .map(|(k, v)| (k.to_string(), v.to_string()))
        .collect()
}

/// One step of the plan. The live runner matches on this enum exhaustively,
/// so every planned step has code behind it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Step {
    // deploy-testnet
    CheckRoot,
    CreateAccounts,
    DeployXusd,
    MintXusd,
    DeriveBaseAddresses,
    DeployAgents,
    RunnerKeys,
    LockTranslator,
    SeedWorld,
    // G1: key handover on a real node
    G1Deploy,
    G1Sell,
    G1OldKeysFail,
    // G2: Chain Signatures Base wallet
    G2Derive,
    G2Sign,
    G2Relay,
    // G6: NEAR AI Cloud signature verified by the contract
    G6Completion,
    G6Signature,
    G6OnChain,
    // scenes (demo order 1-6; recorded in the order 3, 4, 5, 1, 2, 6)
    Scene1BreakIn,
    Scene2Business,
    Scene3Verified,
    Scene4Cheat,
    Scene5Sale,
    Scene6AgentBuysAgent,
}

pub struct StepInfo {
    pub step: Step,
    pub id: &'static str,
    pub what: &'static str,
    /// Secrets the step needs (names).
    pub needs: &'static [&'static str],
    /// Files it writes under out/.
    pub writes: &'static str,
}

pub fn info(step: Step) -> StepInfo {
    use Step::*;
    let (id, what, needs, writes): (&str, &str, &[&str], &str) = match step {
        CheckRoot => ("check-root", "read ROOT's balance and keys on testnet (both RPCs); refuse below the NEAR the plan needs", &["AGENT_EXIT_ROOT_KEY"], ""),
        CreateAccounts => ("create-accounts", "create sub-accounts of ROOT: xusd, translator, cheat, summarizer, alice, bob, carol, p1-p6, x.alice (each skipped if it exists); new keys go to ~/.agent-exit/keys (0600)", &["AGENT_EXIT_ROOT_KEY"], "out/testnet/accounts.json"),
        DeployXusd => ("deploy-xusd", "deploy the FAKE test dollar to xusd.ROOT, new(admin_id = ROOT)", &[], "out/testnet/deploy.json"),
        MintXusd => ("mint-xusd", "mint xUSD-FAKE: bob, carol, summarizer 2,000.00; p1-p6 200.00; alice, x.alice 300.00; register both agents", &["AGENT_EXIT_ROOT_KEY"], "out/testnet/deploy.json"),
        DeriveBaseAddresses => ("derive-base", "read v1.signer-prod.testnet public_key; derive each agent's Base Sepolia address for path wallet-1 (tools/base-wallet)", &[], "out/testnet/deploy.json"),
        DeployAgents => ("deploy-agents", "deploy target/near-testnet/agent_exit.wasm to translator.ROOT and cheat.ROOT, new(owner_id = alice, base_address); check get_status token/admin/mpc and the code hash", &[], "out/testnet/deploy.json"),
        RunnerKeys => ("runner-keys", "alice sets a runner key on both agents (set_runner_key, 1 yocto); wait for runner_key_confirmed", &[], "out/testnet/deploy.json"),
        LockTranslator => ("lock-translator", "delete translator's setup full-access key (locked account); cheat keeps its own (the planted backdoor)", &[], "out/testnet/deploy.json"),
        SeedWorld => ("seed-world", "fixed world: p1-p6 pay translator 5 x 12.00 each, alice 2 x 50.00, x.alice 40.00; alice and x.alice pay cheat 500.00", &[], "out/testnet/seed.json"),
        G1Deploy => ("g1-deploy", "fresh agent g1-<time>.ROOT: deploy, new(owner alice), runner key, delete the setup key, list at 10.00", &["AGENT_EXIT_ROOT_KEY"], "out/gates/g1.json"),
        G1Sell => ("g1-sell", "bob buys it with one ft_transfer_call; key list before/after", &[], "out/gates/g1.json"),
        G1OldKeysFail => ("g1-old-keys", "alice's old runner key and the deleted setup key each sign a heartbeat: the node's real error (AccessKeyNotFound) is recorded", &[], "out/gates/g1.json"),
        G2Derive => ("g2-derive", "derive translator's Base address from the live MPC key; compare with get_base_wallet.address", &[], "out/gates/g2-sign.json"),
        G2Sign => ("g2-sign", "alice sets Base limits (cap 5.00 USDC, payee BASE_PAYEE or the relayer); her runner calls pay_base 5 times (0.01 USDC, valid 300 s): in-contract MPC sign, latency per sample, every signature checked offline", &[], "out/gates/g2-sign.json"),
        G2Relay => ("g2-relay", "scripts/testnet/g2-relay.mjs: chainsig.js derives the same address; viem checks chain id 84532, USDC DOMAIN_SEPARATOR and balance; the relayer EOA broadcasts transferWithAuthorization (fund the derived address with Base Sepolia USDC first: Circle faucet)", &["BASE_RELAYER_KEY"], "out/gates/g2.json"),
        G6Completion => ("g6-completion", "one NEAR AI Cloud chat completion whose request names the agent, job, payer and amount (metadata)", &["NEARAI_API_KEY"], "out/gates/g6.json"),
        G6Signature => ("g6-signature", "GET /v1/signature/{chat_id}?model=&signing_algo=ecdsa; check text = model:sha256(request):sha256(response) and recover the signer offline (EIP-191)", &["NEARAI_API_KEY"], "out/gates/g6.json"),
        G6OnChain => ("g6-on-chain", "ROOT (registry admin) add_signer(signing_address); p1 pays translator 1.00 with the proof; the payment event must say verified: true", &["AGENT_EXIT_ROOT_KEY"], "out/gates/g6.json"),
        Scene1BreakIn => ("scene-1", "break-in: alice calls withdraw, set_runner_key, list_for_sale, set_limits, nft_transfer, pay_base (each refused); her old runner key and the deleted setup key fail at the node; key list before/after", &[], "out/scenes/scene-1.json"),
        Scene2Business => ("scene-2", "business: bob sets limits; p1, p2 pay 25.00 (split 20/5); bob's runner pays summarizer 8.00; 45.00 fails PolicyViolation:DailyCap", &[], "out/scenes/scene-2.json"),
        Scene3Verified => ("scene-3", "verified vs unverified: get_earnings of translator and cheat side by side", &[], "out/scenes/scene-3.json"),
        Scene4Cheat => ("scene-4", "cheat caught: alice lists cheat at 500.00; key list from both RPCs + get_status -> scan-input.v1 -> scan: blocked full_access_key_present; no buy is sent", &[], "out/scenes/scene-4.json"),
        Scene5Sale => ("scene-5", "honest sale: waits until no Base authorization is open (A9); alice lists translator at 1,000.00; bob scans translator (clean) and buys in one ft_transfer_call; keys before/after; alice +700.00, escrow 300.00", &[], "out/scenes/scene-5.json"),
        Scene6AgentBuysAgent => ("scene-6", "agent buys agent: bob lists translator at 1,200.00; summarizer (an agent account) scans it and buys it, then checks bob is locked out (scripted client; MCP testnet writes are not wired yet)", &[], "out/scenes/scene-6.json"),
    };
    StepInfo {
        step,
        id,
        what,
        needs,
        writes,
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Command {
    Deploy,
    Gates,
    GateG1,
    GateG2,
    GateG6,
    Scenes,
    Scene(u8),
}

impl Command {
    pub fn parse(s: &str) -> Option<Command> {
        Some(match s {
            "deploy" => Command::Deploy,
            "gates" => Command::Gates,
            "g1" => Command::GateG1,
            "g2" => Command::GateG2,
            "g6" => Command::GateG6,
            "scenes" => Command::Scenes,
            _ => Command::Scene(
                s.strip_prefix("scene-")?
                    .parse()
                    .ok()
                    .filter(|n| (1..=6).contains(n))?,
            ),
        })
    }
}

/// Scene order for recording: the sale (5) must happen before the break-in (1).
pub const RECORDING_ORDER: [u8; 6] = [3, 4, 5, 1, 2, 6];

pub fn scene_step(n: u8) -> Step {
    match n {
        1 => Step::Scene1BreakIn,
        2 => Step::Scene2Business,
        3 => Step::Scene3Verified,
        4 => Step::Scene4Cheat,
        5 => Step::Scene5Sale,
        _ => Step::Scene6AgentBuysAgent,
    }
}

pub fn plan(cmd: Command) -> Vec<Step> {
    use Step::*;
    match cmd {
        Command::Deploy => vec![
            CheckRoot,
            CreateAccounts,
            DeployXusd,
            MintXusd,
            DeriveBaseAddresses,
            DeployAgents,
            RunnerKeys,
            LockTranslator,
            SeedWorld,
        ],
        Command::GateG1 => vec![CheckRoot, G1Deploy, G1Sell, G1OldKeysFail],
        Command::GateG2 => vec![G2Derive, G2Sign, G2Relay],
        Command::GateG6 => vec![G6Completion, G6Signature, G6OnChain],
        Command::Gates => [
            plan(Command::GateG1),
            plan(Command::GateG6),
            plan(Command::GateG2),
        ]
        .concat(),
        Command::Scenes => RECORDING_ORDER.iter().map(|n| scene_step(*n)).collect(),
        Command::Scene(n) => vec![scene_step(n)],
    }
}

/// NEAR each created account gets from ROOT (agents hold their wasm's storage).
pub const FUNDING: [(&str, u32); 15] = [
    ("xusd", 4),
    ("translator", 11),
    ("cheat", 11),
    ("summarizer", 2),
    ("alice", 3),
    ("bob", 3),
    ("carol", 2),
    ("p1", 1),
    ("p2", 1),
    ("p3", 1),
    ("p4", 1),
    ("p5", 1),
    ("p6", 1),
    ("x.alice", 1),
    ("g1 agent (per run)", 7),
];

pub fn plan_json(cmd: Command, cfg: &Config, env_found: bool) -> Value {
    let steps: Vec<Value> = plan(cmd)
        .into_iter()
        .map(|s| {
            let i = info(s);
            let missing: Vec<&str> = i.needs.iter().copied().filter(|n| !cfg.has(n)).collect();
            json!({"id": i.id, "what": i.what.replace("ROOT", &cfg.root), "needs": i.needs,
                   "missing": missing, "writes": i.writes})
        })
        .collect();
    json!({
        "schema": "testnet-plan.v1",
        "dry_run": true,
        "network": NETWORK,
        "command": format!("{cmd:?}"),
        "env_file_found": env_found,
        "root": cfg.root,
        "rpc": cfg.rpc,
        "base_rpc": cfg.base_rpc,
        "mpc_signer": MPC_SIGNER,
        "base_chain_id": BASE_CHAIN_ID,
        "secrets_present": cfg.secrets_present,
        "near_needed": FUNDING.iter().map(|(_, n)| n).sum::<u32>() + 3,
        "steps": steps,
        "money": "xUSD-FAKE only; Base Sepolia test USDC; never mainnet",
    })
}

pub fn plan_text(v: &Value) -> String {
    let mut s = format!(
        "DRY RUN ({}): nothing is sent. Network testnet, root {}, RPC {}.\n",
        v["command"].as_str().unwrap_or(""),
        v["root"].as_str().unwrap_or(""),
        v["rpc"][0].as_str().unwrap_or("")
    );
    if v["env_file_found"] == false {
        s.push_str("No env file found: showing the defaults. Create ~/.agent-exit.env (see .env.example).\n");
    }
    for (i, st) in v["steps"].as_array().into_iter().flatten().enumerate() {
        s.push_str(&format!(
            "{:>2}. {:<16} {}\n",
            i + 1,
            st["id"].as_str().unwrap_or(""),
            st["what"].as_str().unwrap_or("")
        ));
        let missing = st["missing"].as_array().map(|m| m.len()).unwrap_or(0);
        if missing > 0 {
            s.push_str(&format!("    needs (missing): {}\n", st["missing"]));
        }
        if let Some(w) = st["writes"].as_str().filter(|w| !w.is_empty()) {
            s.push_str(&format!("    writes {w}\n"));
        }
    }
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    fn env(pairs: &[(&str, &str)]) -> BTreeMap<String, String> {
        pairs
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect()
    }

    #[test]
    fn parses_env_files() {
        let m = parse_env("# c\nexport A=1\nB = \"two\"\nC='3'\n\nA=4\nnot a line\n");
        assert_eq!(m["A"], "4");
        assert_eq!(m["B"], "two");
        assert_eq!(m["C"], "3");
        assert_eq!(m.len(), 3);
    }

    #[test]
    fn refuses_anything_but_testnet() {
        assert!(check(&env(&[])).is_err());
        let bad = [
            ("AGENT_EXIT_NETWORK", "mainnet"),
            ("AGENT_EXIT_ROOT", "agentexit.near"),
            ("AGENT_EXIT_RPC", "https://rpc.mainnet.near.org"),
            ("AGENT_EXIT_RPC", "http://rpc.testnet.near.org"),
            (
                "AGENT_EXIT_RPC",
                "https://evil.example/rpc.testnet.near.org",
            ),
            ("AGENT_EXIT_RPC", "https://user@rpc.testnet.near.org"),
            ("AGENT_EXIT_RPC_2", "https://rpc.testnet.near.org:8443"),
            ("BASE_SEPOLIA_RPC", "https://mainnet.base.org"),
            ("NEARAI_BASE_URL", "https://example.com"),
            ("BASE_PAYEE", "0x12"),
        ];
        for (k, v) in bad {
            let mut e = default_env();
            e.insert(k.into(), v.into());
            assert!(check(&e).is_err(), "{k}={v} must be refused");
        }
        let c = check(&default_env()).unwrap();
        assert_eq!(c.root, DEFAULT_ROOT);
        assert_eq!(c.agent(), "translator.agentexit.testnet");
        assert!(c.secrets_present.is_empty());
    }

    #[test]
    fn dry_run_never_prints_secrets() {
        let mut e = default_env();
        e.insert("AGENT_EXIT_ROOT_KEY".into(), "ed25519:SECRETSECRET".into());
        e.insert("NEARAI_API_KEY".into(), "sk-SECRETSECRET".into());
        e.insert("BASE_RELAYER_KEY".into(), "0xSECRETSECRET".into());
        let c = check(&e).unwrap();
        for cmd in [Command::Deploy, Command::Gates, Command::Scenes] {
            let v = plan_json(cmd, &c, true);
            let out = format!("{}{}", v, plan_text(&v));
            assert!(!out.contains("SECRET"), "{out}");
            assert!(out.contains("AGENT_EXIT_ROOT_KEY"));
        }
    }

    #[test]
    fn plans_cover_the_testnet_day() {
        let c = check(&default_env()).unwrap();
        let ids = |cmd| -> Vec<String> {
            plan_json(cmd, &c, false)["steps"]
                .as_array()
                .unwrap()
                .iter()
                .map(|s| s["id"].as_str().unwrap().to_string())
                .collect()
        };
        assert_eq!(ids(Command::Deploy).len(), 9);
        assert_eq!(
            ids(Command::Gates),
            [
                "check-root",
                "g1-deploy",
                "g1-sell",
                "g1-old-keys",
                "g6-completion",
                "g6-signature",
                "g6-on-chain",
                "g2-derive",
                "g2-sign",
                "g2-relay"
            ]
        );
        assert_eq!(
            ids(Command::Scenes),
            ["scene-3", "scene-4", "scene-5", "scene-1", "scene-2", "scene-6"]
        );
        assert_eq!(Command::parse("scene-4"), Some(Command::Scene(4)));
        assert_eq!(Command::parse("scene-7"), None);
        let missing = &plan_json(Command::GateG6, &c, false)["steps"][0]["missing"];
        assert_eq!(missing, &json!(["NEARAI_API_KEY"]));
    }
}
