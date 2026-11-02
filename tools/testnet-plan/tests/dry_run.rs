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
    p.to_string_lossy().into()
}

#[test]
fn dry_run_without_an_env_file_prints_the_plan() {
    for (cmd, want) in [
        ("deploy", "lock-translator"),
        ("gates", "g2-relay"),
        ("scenes", "scene-6"),
        ("g6", "g6-on-chain"),
        ("scene-1", "break-in"),
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
