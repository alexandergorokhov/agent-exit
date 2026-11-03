//! `testnet-plan <deploy|gates|g1|g2|g6|scenes|scene-N> [--json]`
//! Prints what the live runner would do. Offline: reads ~/.agent-exit.env
//! (or AGENT_EXIT_ENV) only to check it and to list which secrets are
//! present, by name. Exit 1 if the env file asks for anything but testnet.

use testnet_plan::*;

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let Some(cmd) = args.first().and_then(|a| Command::parse(a)) else {
        eprintln!("usage: testnet-plan <deploy|gates|g1|g2|g6|scenes|scene-1..scene-7> [--json]");
        std::process::exit(2);
    };
    let path = env_path();
    if let Err(e) = check_secret_mode(&path) {
        eprintln!("refused: {e}");
        std::process::exit(1);
    }
    let (env, found) = match std::fs::read_to_string(&path) {
        Ok(t) => (parse_env(&t), true),
        Err(_) => (default_env(), false),
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
    let v = plan_json(cmd, &cfg, found);
    if args.iter().any(|a| a == "--json") {
        println!("{}", serde_json::to_string_pretty(&v).unwrap());
    } else {
        print!("{}", plan_text(&v));
    }
}
