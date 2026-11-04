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
    for t in ["attack-sandbox", "e2e", "gate-g0", "record"] {
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
    let fams = v["families"].as_array().unwrap();
    let all: u64 = fams.iter().map(|f| f["cases"].as_u64().unwrap()).sum();
    assert_eq!(all, 200);
    assert!(fams.iter().all(|f| f["missed"] == 0));
    let counted: u64 = fams
        .iter()
        .filter(|f| f["counted"] == true)
        .map(|f| f["cases"].as_u64().unwrap())
        .sum();
    assert_eq!(v["in_scope"]["cases"], counted);
    assert_eq!(v["in_scope"]["missed"], 0);
    assert_eq!(v["not_counted"][0]["id"], "A9");
    assert!(v["summary"]["text"]
        .as_str()
        .unwrap()
        .ends_with("each blocked by its rule; 0/50 false positives (regression suite)"));
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
    assert!(
        !page.contains("coming after deploy"),
        "the testnet run happened; the page must not say it is coming"
    );
    // Every NearBlocks link the page shows comes from the recorded testnet run.
    let data = std::fs::read_to_string(root().join("web/data/testnet.json")).unwrap();
    let mut recorded = String::new();
    for e in std::fs::read_dir(root().join("out/scenes")).unwrap() {
        recorded.push_str(&std::fs::read_to_string(e.unwrap().path()).unwrap());
    }
    let mut n = 0;
    for part in data.split("testnet.nearblocks.io/txns/").skip(1) {
        let h: String = part
            .chars()
            .take_while(|c| c.is_ascii_alphanumeric())
            .collect();
        assert!(recorded.contains(&h), "page links {h}, not in out/scenes");
        n += 1;
    }
    assert!(n >= 10, "page should link the testnet run per step ({n})");
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
    for t in ["attack-sandbox", "attack-testnet", "record", "e2e"] {
        let line = help
            .lines()
            .find(|l| l.starts_with(&format!("{t}\t")))
            .unwrap_or_else(|| panic!("help lacks {t}"));
        assert!(
            line.contains("(not built yet)"),
            "help line for {t}: {line}"
        );
    }
    for t in [
        "demo",
        "demo-setup",
        "verify",
        "attack",
        "web-test",
        "mcp-test",
    ] {
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
    // The counts come from the golden attack report, so they cannot go stale.
    let g: serde_json::Value = serde_json::from_str(&read("tests/golden/attack.json")).unwrap();
    let total = g["families"].as_array().unwrap().len();
    let counted = g["summary"]["families"].as_u64().unwrap();
    let claim = format!(
        "{total} attack families, {counted} counted (A9 Base not counted until live); each blocked by its rule"
    );
    for f in ["README.md", "AGENTS.md", "llms.txt"] {
        let t = read(f);
        assert!(
            t.contains(&claim),
            "{f}: missing the claim built from the golden: {claim}"
        );
        assert!(
            t.contains("regression suite, not independent evidence"),
            "{f}"
        );
    }
    let r = read("README.md");
    assert!(r.contains("A9"), "README: A9 excluded");
    // A5 is honest about its hash: the reproducible-build hash once
    // contracts/res/SHA256SUMS is committed, a placeholder until then.
    if std::path::Path::new(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../contracts/res/SHA256SUMS"
    ))
    .exists()
    {
        assert!(
            r.contains("SHA256SUMS") && !r.contains("placeholder hash"),
            "README: A5 checks the reproducible-build hash (contracts/res/SHA256SUMS)"
        );
    } else {
        assert!(r.contains("placeholder hash"), "README: A5 placeholder");
    }
}

#[test]
fn s22_readme_hero_comes_first() {
    let r = read("README.md");
    let at = |s: &str| r.find(s).unwrap_or_else(|| panic!("README lacks {s}"));
    let fold = at("## The problem, and what the contract does");
    assert!(at("make demo") < fold);
    assert!(at("```mermaid") < fold);
    assert!(at("609b135") < fold && at("63d597c") < fold);
    assert!(at("Live on NEAR testnet") < fold);
    assert!(
        !r[..fold].contains("| Claim |"),
        "claims table must be below the fold"
    );
    let hero = &r[..at("## The problem, and what the contract does")];
    assert!(
        hero.lines().count() < 100, // includes the "try it in 2 minutes" table
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
    // 'wasm-unsafe-eval' lets the page compile its own playground.wasm (same
    // origin); it does not allow JS eval. Nothing else "unsafe" is allowed.
    let rest = csp.replace("'wasm-unsafe-eval'", "");
    for bad in ["unsafe-inline", "unsafe-eval", "cdn.", "*", "data:"] {
        assert!(!rest.contains(bad), "CSP must not contain {bad}: {csp}");
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

/// S25: the docs say how to pay an agent (storage_deposit first, A49), tell
/// the A47 real-node finding honestly, and list A43-A49 in the scenarios.
#[test]
fn s25_docs_explain_paying_and_the_found_bug() {
    let readme = read("README.md");
    for f in ["README.md", "AGENTS.md", "llms.txt"] {
        let t = read(f);
        assert!(t.contains("storage_deposit"), "{f}: how to pay");
        assert!(t.contains("0.024 NEAR"), "{f}: the payer row price");
    }
    assert!(
        !readme.contains("no NEP-145"),
        "the old no-deposit claim is gone"
    );
    assert!(readme.contains("Found and fixed on a real node"));
    assert!(readme.contains("a47_thousand_long_payers_then_buy_and_views"));
    assert!(
        readme.contains("unconfirmed"),
        "the cause is not overclaimed"
    );
    let sc = read("docs/SCENARIOS.md");
    for id in ["A43", "A44", "A45", "A46", "A47", "A48", "A49"] {
        assert!(sc.contains(&format!("| {id} |")), "{id} is listed");
    }
}

/// The MCP test count in the docs is one number everywhere and never below
/// the number of top-level `test(` declarations (some tests are generated, so
/// the real count, from `make mcp-test`, can be higher).
#[test]
fn s25_mcp_test_count_is_one_number_and_not_below_the_files() {
    let mut declared = 0;
    for e in std::fs::read_dir(format!("{}/../../mcp/test", env!("CARGO_MANIFEST_DIR"))).unwrap() {
        let p = e.unwrap().path();
        if p.to_string_lossy().ends_with(".test.ts") {
            declared += std::fs::read_to_string(p)
                .unwrap()
                .lines()
                .filter(|l| l.starts_with("test("))
                .count();
        }
    }
    let re = |t: &str| -> Vec<usize> {
        t.match_indices(" MCP tests")
            .filter_map(|(i, _)| {
                let d: String = t[..i]
                    .chars()
                    .rev()
                    .take_while(|c| c.is_ascii_digit())
                    .collect();
                (!d.is_empty()).then(|| d.chars().rev().collect::<String>().parse().unwrap())
            })
            .collect()
    };
    for f in ["README.md", "web/index.html"] {
        for n in re(&read(f)) {
            assert!(
                n >= declared,
                "{f}: {n} MCP tests but {declared} are declared"
            );
            assert_eq!(n, 86, "{f}: the MCP test count differs from make mcp-test");
        }
    }
}

/// S25-S28: the README first screen, the wording fixes and the demo summary.
#[test]
fn s25_readme_first_screen() {
    let r = read("README.md");
    let first_screen = &r[..r.find("## One sale").unwrap()];
    for want in [
        "docs/img/demo.png",
        "Why only on NEAR",
        "make demo",
        "docs/PROOF.md",
    ] {
        assert!(
            first_screen.contains(want),
            "README first screen lacks {want}"
        );
    }
    assert!(
        !r.contains("## What is proven, and where"),
        "the proof table moved out of the README"
    );
    assert!(read("docs/PROOF.md").contains("| Claim |"));
    assert!(std::path::Path::new(&format!(
        "{}/../../docs/img/demo.png",
        env!("CARGO_MANIFEST_DIR")
    ))
    .exists());
    for f in [
        "README.md",
        "AGENTS.md",
        "llms.txt",
        "web/index.html",
        "web/data/demo.json",
        "web/app.js",
    ] {
        let t = read(f).to_lowercase();
        assert!(
            !t.contains("all or nothing") && !t.contains("all-or-nothing"),
            "{f}: all-or-nothing wording"
        );
        assert!(
            !t.contains("ai agent marketplace"),
            "{f}: marketplace label"
        );
    }
    assert!(read("web/index.html").contains("Buy an AI agent with real earnings"));
    assert!(read("web/index.html").contains("Try it yourself"));
    assert!(read("web/index.html").contains("Why only on NEAR"));
    assert!(read(".gitignore").lines().any(|l| l == ".claude/"));
    let mk = read("Makefile");
    assert!(mk.contains("scripts/demo-summary.mjs") && mk.contains("-p verify -- --json --out $(OUT)/verify.json --golden tests/golden/verify.json > /dev/null"));
}

/// Every NearBlocks transaction the README links was recorded by the testnet
/// runner (out/testnet, out/gates, out/scenes): no invented links.
#[test]
fn readme_testnet_links_are_recorded_runs() {
    let r = read("README.md");
    let mut recorded = String::new();
    for dir in ["out/testnet", "out/gates", "out/scenes"] {
        let p = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../..")
            .join(dir);
        for e in std::fs::read_dir(&p).unwrap_or_else(|_| panic!("{dir} missing")) {
            recorded.push_str(&std::fs::read_to_string(e.unwrap().path()).unwrap());
        }
    }
    // The AI buyer's testnet run records its own transactions.
    let llm =
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../out/llm-buyer-testnet.json");
    if let Ok(t) = std::fs::read_to_string(llm) {
        recorded.push_str(&t);
    }
    let mut n = 0;
    for part in r.split("testnet.nearblocks.io/txns/").skip(1) {
        let hash: String = part
            .chars()
            .take_while(|c| c.is_ascii_alphanumeric())
            .collect();
        assert!(
            recorded.contains(&hash),
            "README links tx {hash}, not in out/"
        );
        n += 1;
    }
    assert!(
        n >= 5,
        "README should link the live testnet runs ({n} links)"
    );
}
