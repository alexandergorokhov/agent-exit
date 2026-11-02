//! S17 (Rust part): the AI-judge entry points point at files that exist, and
//! unbuilt Makefile targets fail loudly instead of faking success.

use std::path::Path;
use std::process::Command;

fn root() -> &'static Path {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .parent()
        .unwrap()
}

fn links(file: &str) -> Vec<String> {
    let text = std::fs::read_to_string(root().join(file)).unwrap();
    text.split("](")
        .skip(1)
        .filter_map(|s| s.split(')').next())
        .filter(|l| !l.starts_with("http") && !l.starts_with('#'))
        .map(String::from)
        .collect()
}

#[test]
fn s17_llms_txt_paths_exist() {
    let l = links("llms.txt");
    assert!(l.len() >= 10, "llms.txt lists too few files: {l:?}");
    for p in l {
        assert!(root().join(&p).exists(), "llms.txt names missing path {p}");
    }
}

#[test]
fn s17_readme_and_agents_links_exist() {
    for f in ["README.md", "AGENTS.md"] {
        for p in links(f) {
            assert!(root().join(&p).exists(), "{f} links missing path {p}");
        }
    }
}

#[test]
fn s17_agents_md_paths_exist() {
    let text = std::fs::read_to_string(root().join("AGENTS.md")).unwrap();
    for word in text.split('`') {
        let w = word.split(':').next().unwrap();
        if (w.starts_with("contracts/")
            || w.starts_with("tools/")
            || w.starts_with("tests/")
            || w.starts_with("fixtures/"))
            && !w.contains(' ')
        {
            assert!(root().join(w).exists(), "AGENTS.md names missing path {w}");
        }
    }
}

#[test]
fn unbuilt_make_targets_fail_loudly() {
    if Command::new("make").arg("--version").output().is_err() {
        eprintln!("make not installed; skipped");
        return;
    }
    for t in [
        "attack-sandbox",
        "fuzz",
        "web-deploy",
        "e2e",
        "scene-1",
        "gate-g1",
    ] {
        let out = Command::new("make")
            .arg("-s")
            .arg(t)
            .current_dir(root())
            .output()
            .unwrap();
        assert!(!out.status.success(), "make {t} must fail until built");
        let err = String::from_utf8_lossy(&out.stderr);
        assert!(err.contains("not built yet"), "make {t}: {err}");
    }
}

#[test]
fn golden_is_valid_verify_output() {
    let v: serde_json::Value = serde_json::from_str(
        &std::fs::read_to_string(root().join("tests/golden/verify.json")).unwrap(),
    )
    .unwrap();
    assert_eq!(v["schema"], "verify-output.v1");
    assert_eq!(v["summary"]["failed"], 0);
}

#[test]
fn signature_fixtures_are_reproducible_and_labelled() {
    let out = Command::new(env!("CARGO_BIN_EXE_gen-test-signatures"))
        .arg("--check")
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let text =
        std::fs::read_to_string(root().join("fixtures/test-signatures/translator.json")).unwrap();
    assert!(text.contains("TEST FIXTURE") && text.contains("NOT a NEAR AI Cloud signature"));
}

#[test]
fn golden_attack_report_blocks_everything_in_scope() {
    let v: serde_json::Value = serde_json::from_str(
        &std::fs::read_to_string(root().join("tests/golden/attack.json")).unwrap(),
    )
    .unwrap();
    assert_eq!(v["schema"], "attack-report.v1");
    assert_eq!(v["seed"], 42);
    assert_eq!(v["in_scope"]["cases"], 200);
    assert_eq!(v["in_scope"]["missed"], 0);
    assert_eq!(v["clean"]["cases"], 50);
    assert_eq!(v["clean"]["false_positives"], 0);
    assert_eq!(v["not_caught_by_design"].as_array().unwrap().len(), 6);
}

#[test]
fn mcp_world_fixture_is_recorded_from_the_contract() {
    let out = Command::new(env!("CARGO_BIN_EXE_gen-mcp-world"))
        .arg("--check")
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let v: serde_json::Value = serde_json::from_str(
        &std::fs::read_to_string(root().join("fixtures/mcp/world.json")).unwrap(),
    )
    .unwrap();
    assert_eq!(v["schema"], "mcp-world.v1");
    assert_eq!(v["network"], "offline");
    assert!(v["_note"].as_str().unwrap().contains("xUSD-FAKE"));
}

#[test]
fn web_demo_data_is_recorded_from_the_contract() {
    let out = Command::new(env!("CARGO_BIN_EXE_gen-web-demo"))
        .arg("--check")
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let v: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(root().join("web/data/demo.json")).unwrap())
            .unwrap();
    assert_eq!(v["schema"], "web-demo.v1");
    assert!(v["money"].as_str().unwrap().contains("xUSD-FAKE"));
    let ids: Vec<&str> = v["steps"]
        .as_array()
        .unwrap()
        .iter()
        .map(|s| s["id"].as_str().unwrap())
        .collect();
    // The demo order in AGENT-EXIT.md.
    assert_eq!(
        ids,
        [
            "break-in",
            "business",
            "verified",
            "cheat",
            "honest-sale",
            "agent-buys-agent"
        ]
    );
    let page = std::fs::read_to_string(root().join("web/app.js")).unwrap();
    assert!(page.contains("testnet: coming after deploy"));
    assert!(
        !page.contains("mainnet"),
        "the page never names a mainnet network"
    );
}

#[test]
fn golden_agent_buys_agent_passes() {
    let v: serde_json::Value = serde_json::from_str(
        &std::fs::read_to_string(root().join("tests/golden/agent-buys-agent.json")).unwrap(),
    )
    .unwrap();
    assert_eq!(v["schema"], "agent-buys-agent.v1");
    assert_eq!(v["pass"], true);
    assert_eq!(v["summary"]["cheat_buy_refused"], true);
    assert_eq!(v["summary"]["seller_locked_out_after"], true);
}
