//! The dry run of `make deploy-testnet`, `make gates` and `make scenes`:
//! offline, with or without an env file; refuses a non-testnet env file;
//! never prints a secret.

use std::process::Command;

fn run(env_file: &str, args: &[&str]) -> (bool, String, String) {
    let o = Command::new(env!("CARGO_BIN_EXE_testnet-plan"))
        .args(args)
        .env("AGENT_EXIT_ENV", env_file)
        .output()
        .unwrap();
    (
        o.status.success(),
        String::from_utf8_lossy(&o.stdout).into(),
        String::from_utf8_lossy(&o.stderr).into(),
    )
}

fn tmp(name: &str, text: &str) -> String {
    let dir = std::env::temp_dir().join(format!("agent-exit-plan-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let p = dir.join(name);
    std::fs::write(&p, text).unwrap();
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(&p, std::fs::Permissions::from_mode(0o600)).unwrap();
    p.to_string_lossy().into()
}

#[test]
fn dry_run_refuses_a_group_readable_env_file() {
    let p = tmp(
        "loose.env",
        "AGENT_EXIT_ROOT_KEY=ed25519:secret-value-xyz\n",
    );
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(&p, std::fs::Permissions::from_mode(0o644)).unwrap();
    let (ok, out, err) = run(&p, &["deploy"]);
    assert!(!ok);
    assert!(err.contains("chmod 600"), "{err}");
    assert!(!out.contains("secret-value-xyz") && !err.contains("secret-value-xyz"));
}

#[test]
fn dry_run_without_an_env_file_prints_the_plan() {
    for (cmd, want) in [
        ("deploy", "lock-translator"),
        ("gates", "g2-relay"),
        ("scenes", "scene-6"),
        ("g6", "g6-on-chain"),
        ("scene-1", "break-in"),
        ("scene-7", "AI buyer setup"),
    ] {
        let (ok, out, _) = run("/nonexistent/agent-exit.env", &[cmd]);
        assert!(ok, "{cmd}");
        assert!(out.starts_with("DRY RUN"), "{out}");
        assert!(out.contains(want), "{cmd}: {out}");
        assert!(out.contains("No env file found"));
    }
    let (ok, out, _) = run("/nonexistent", &["scenes", "--json"]);
    assert!(ok);
    let v: serde_json::Value = serde_json::from_str(&out).unwrap();
    assert_eq!(v["schema"], "testnet-plan.v1");
    assert_eq!(v["network"], "testnet");
    assert_eq!(v["steps"].as_array().unwrap().len(), 6);
}

#[test]
fn dry_run_refuses_a_mainnet_env_file() {
    for text in [
        "AGENT_EXIT_NETWORK=mainnet\n",
        "AGENT_EXIT_NETWORK=testnet\nAGENT_EXIT_ROOT=agentexit.near\n",
        "AGENT_EXIT_NETWORK=testnet\nAGENT_EXIT_RPC=https://rpc.mainnet.near.org\n",
    ] {
        let f = tmp("bad.env", text);
        let (ok, _, err) = run(&f, &["deploy"]);
        assert!(!ok, "{text}");
        assert!(err.contains("refused"), "{err}");
    }
}

#[test]
fn dry_run_names_secrets_but_never_prints_them() {
    let f = tmp(
        "good.env",
        "AGENT_EXIT_NETWORK=testnet\nAGENT_EXIT_ROOT=demo.testnet\nAGENT_EXIT_ROOT_KEY=ed25519:TOPSECRETVALUE\nNEARAI_API_KEY=sk-TOPSECRETVALUE\nBASE_RELAYER_KEY=0xTOPSECRETVALUE\n",
    );
    for cmd in ["deploy", "gates", "scenes"] {
        for extra in [None, Some("--json")] {
            let mut args = vec![cmd];
            args.extend(extra);
            let (ok, out, err) = run(&f, &args);
            assert!(ok, "{err}");
            assert!(!out.contains("TOPSECRET") && !err.contains("TOPSECRET"));
            assert!(out.contains("demo.testnet"));
        }
    }
    let (_, out, _) = run(&f, &["gates", "--json"]);
    let v: serde_json::Value = serde_json::from_str(&out).unwrap();
    assert_eq!(
        v["secrets_present"],
        serde_json::json!(["AGENT_EXIT_ROOT_KEY", "NEARAI_API_KEY", "BASE_RELAYER_KEY"])
    );
    assert!(v["steps"]
        .as_array()
        .unwrap()
        .iter()
        .all(|s| s["missing"].as_array().unwrap().is_empty()));
}

/// Lean funding (the faucet gives 2 NEAR a request): the deploy fits in
/// about 30 NEAR, yet each agent account still covers its code, the growth
/// reserve its contract demands (512 signature + 400 day rows of 200 bytes)
/// and the 1 NEAR reserve, or the scan would call it insolvent (A46).
#[test]
fn lean_funding_still_keeps_agents_solvent() {
    use testnet_plan::{deploy_need_milli, funding_milli, G1_AGENT_MILLI};
    let wasm = std::fs::metadata(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../contracts/res/agent_exit.wasm"
    ))
    .map(|m| m.len() as u128)
    .unwrap_or(522_403);
    // 1e19 yocto per byte = 1e-5 NEAR = 0.01 milliNEAR per byte.
    let need_milli = (wasm + 8_000 /* state */ + (512 + 400) * 200) / 100 + 1_000;
    for a in ["translator", "cheat"] {
        let f = funding_milli(a).expect(a) as u128;
        assert!(
            f >= need_milli + 300,
            "{a}: {f} mNEAR < {need_milli} + 300 margin"
        );
    }
    assert!(G1_AGENT_MILLI as u128 >= need_milli + 300);
    let total = deploy_need_milli();
    assert!(
        total <= 30_000,
        "deploy needs {total} mNEAR; keep it under 30 NEAR"
    );
}
