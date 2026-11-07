//! `agent-exit-testnet <deploy|gates|g1|g2|g6|scenes|scene-N> [--dry-run]`
//!
//! The testnet day from one command on the owner's machine. Reads
//! `~/.agent-exit.env` (never prints a value from it), refuses anything but
//! testnet, writes JSON records (tx hashes, explorer links, real errors) to
//! out/testnet, out/gates and out/scenes. `--dry-run` prints the plan and
//! sends nothing. All money is xUSD-FAKE or Base Sepolia test USDC.

mod chain;
mod steps;
mod wasm;

use testnet_plan::*;

#[tokio::main]
async fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let Some(cmd) = args.first().and_then(|a| Command::parse(a)) else {
        eprintln!(
            "usage: agent-exit-testnet <deploy|gates|g1|g2|g6|scenes|scene-1..scene-9> [--dry-run]"
        );
        std::process::exit(2);
    };
    let path = env_path();
    // The env file holds the root full-access key: group- or world-readable is refused.
    if let Err(e) = check_secret_mode(&path) {
        eprintln!("refused: {e}");
        std::process::exit(1);
    }
    let text = std::fs::read_to_string(&path);
    let dry = args.iter().any(|a| a == "--dry-run");
    let env = match (&text, dry) {
        (Ok(t), _) => parse_env(t),
        (Err(_), true) => default_env(),
        (Err(e), false) => {
            eprintln!("{path}: {e}. Copy .env.example there and fill it in (testnet only).");
            std::process::exit(1);
        }
    };
    let cfg = match check(&env) {
        Ok(c) => c,
        Err(errs) => {
            for e in errs {
                eprintln!("refused: {e}");
            }
            std::process::exit(1);
        }
    };
    if dry {
        print!("{}", plan_text(&plan_json(cmd, &cfg, text.is_ok())));
        return;
    }
    let code = match steps::run(cmd, cfg, env).await {
        Ok(()) => 0,
        Err(e) => {
            eprintln!("agent-exit-testnet: {e}");
            1
        }
    };
    std::process::exit(code);
}
