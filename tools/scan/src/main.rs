//! `scan FILE` (or `-` for stdin): read a `scan-input.v1` JSON, print the
//! `scan-report.v1` JSON. Exit 0 clean, 2 blocked, 3 inconclusive, 1 bad input.

use std::io::Read;

fn main() {
    let path = std::env::args().nth(1).unwrap_or_else(|| "-".into());
    let mut text = String::new();
    let read = if path == "-" {
        std::io::stdin().read_to_string(&mut text).map(|_| ())
    } else {
        std::fs::read_to_string(&path).map(|t| text = t)
    };
    if let Err(e) = read {
        eprintln!("scan: cannot read {path}: {e}");
        std::process::exit(1);
    }
    let input: scan::ScanInput = match serde_json::from_str(&text) {
        Ok(i) => i,
        Err(e) => {
            eprintln!("scan: {path} is not scan-input.v1: {e}");
            std::process::exit(1);
        }
    };
    let r = scan::scan(&input);
    println!("{}", serde_json::to_string_pretty(&r).expect("json"));
    std::process::exit(match r.verdict.as_str() {
        "clean" => 0,
        "blocked" => 2,
        _ => 3,
    });
}
