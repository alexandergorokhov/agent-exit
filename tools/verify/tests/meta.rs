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
    for t in ["attack-sandbox", "web-deploy", "e2e", "gate-g0", "record"] {
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

// ---- docs and page honesty (docs/SCENARIOS.md S21-S24, E35) ----

fn read(file: &str) -> String {
    std::fs::read_to_string(root().join(file)).unwrap_or_else(|e| panic!("{file}: {e}"))
}

fn make_out(args: &[&str]) -> (bool, String) {
    let out = Command::new("make")
        .args(args)
        .current_dir(root())
        .output()
        .unwrap();
    (
        out.status.success(),
        format!(
            "{}{}",
            String::from_utf8_lossy(&out.stdout),
            String::from_utf8_lossy(&out.stderr)
        ),
    )
}

#[test]
fn e35_help_marks_unbuilt_targets() {
    let (ok, help) = make_out(&["-s", "help"]);
    assert!(ok);
    for t in [
        "attack-sandbox",
        "attack-testnet",
        "demo-setup",
        "record",
        "web-deploy",
        "e2e",
    ] {
        let line = help
            .lines()
            .find(|l| l.starts_with(&format!("{t}\t")))
            .unwrap_or_else(|| panic!("help lacks {t}"));
        assert!(
            line.contains("(not built yet)"),
            "help line for {t}: {line}"
        );
    }
    for t in ["demo", "verify", "attack", "web-test", "mcp-test"] {
        let line = help
            .lines()
            .find(|l| l.starts_with(&format!("{t}\t")))
            .unwrap();
        assert!(!line.contains("not built yet"), "{t} is built: {line}");
    }
}

#[test]
fn s21_demo_target_runs_the_offline_story() {
    let (ok, plan) = make_out(&["-n", "demo"]);
    assert!(
        ok,
        "make demo must exist and not be a not_built stub: {plan}"
    );
    assert!(!plan.contains("not built yet"), "{plan}");
    let verify = plan.find("verify").expect("demo runs verify");
    let replay = plan
        .find("agent-buys-agent")
        .expect("demo replays agent-buys-agent");
    assert!(verify < replay, "verify subset first, then the replay");
    assert!(
        plan.contains("make web"),
        "demo prints how to open the page"
    );
}

#[test]
fn s22_docs_do_not_overclaim() {
    for f in [
        "README.md",
        "AGENTS.md",
        "llms.txt",
        "web/data/demo.json",
        "web/app.js",
        "web/index.html",
    ] {
        let t = read(f);
        assert!(!t.contains("200/200 caught"), "{f} says 200/200 caught");
        assert!(
            !t.to_lowercase().contains("16 families"),
            "{f}: stale family count"
        );
        assert!(!t.contains("# pass 18"), "{f}: stale MCP test count");
        assert!(!t.contains("18 tests"), "{f}: stale MCP test count");
    }
    for f in ["README.md", "AGENTS.md", "llms.txt"] {
        let t = read(f);
        assert!(
            t.contains("19 attack families"),
            "{f}: missing the reworded claim"
        );
        assert!(
            t.contains("regression suite, not independent evidence"),
            "{f}"
        );
    }
    let r = read("README.md");
    assert!(
        r.contains("A9") && r.contains("placeholder hash"),
        "README: A9 excluded, A5 placeholder"
    );
}

#[test]
fn s22_readme_hero_comes_first() {
    let r = read("README.md");
    let at = |s: &str| r.find(s).unwrap_or_else(|| panic!("README lacks {s}"));
    let fold = at("## What is proven, and where");
    assert!(at("## Try it") < fold);
    assert!(at("```mermaid") < fold);
    assert!(at("609b135") < fold && at("63d597c") < fold);
    assert!(at("Testnet: coming") < fold);
    assert!(
        !r[..fold].contains("| Claim |"),
        "claims table must be below the fold"
    );
    let hero = &r[..at("## The problem, and what the contract does")];
    assert!(
        hero.lines().count() < 60,
        "hero too long: {}",
        hero.lines().count()
    );
}

#[test]
fn s23_page_copy_is_honest() {
    let d = read("web/data/demo.json");
    assert!(
        !d.contains("two RPC providers"),
        "offline copy claims two RPC providers"
    );
    assert!(
        d.contains("FROZEN"),
        "planted key heartbeat must read FROZEN"
    );
    assert!(
        d.contains("counted, not verified"),
        "reused signature label"
    );
    assert!(d.contains("scripted replay"));
    assert!(
        d.contains("real AI buyer transcript"),
        "slot for the AI buyer transcript"
    );
    assert!(
        !d.contains("An agent buys an agent"),
        "offline mode is a scripted replay"
    );
}

#[test]
fn s24_page_has_csp_and_vendored_connector() {
    let h = read("web/index.html");
    let csp = h
        .split("http-equiv=\"Content-Security-Policy\"")
        .nth(1)
        .expect("CSP meta tag")
        .split('>')
        .next()
        .unwrap()
        .to_string();
    assert!(csp.contains("default-src 'none'"), "{csp}");
    assert!(csp.contains("script-src 'self'"), "{csp}");
    for bad in ["unsafe-inline", "unsafe-eval", "cdn.", "*", "data:"] {
        assert!(!csp.contains(bad), "CSP must not contain {bad}: {csp}");
    }
    assert!(!h.contains("<script>"), "no inline scripts");
    let a = read("web/app.js");
    assert!(
        !a.contains("cdn.jsdelivr"),
        "connector must not load from a CDN"
    );
    assert!(a.contains("vendor/"), "connector is vendored");
    assert!(
        root().join("web/vendor/SHA256SUMS").exists(),
        "vendored hash list"
    );
}
