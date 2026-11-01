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
    for t in ["attack", "fuzz", "mcp", "scene-1", "gate-g1"] {
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
