//! `base-wallet derive --mpc-pk secp256k1:... --account a.testnet [--path wallet-1]`
//! `base-wallet derive --mock --account a.testnet`   (the offline TEST signer)
//! `base-wallet check-auth FILE`   (a `base_auth_signed` event as JSON)
//! Offline; prints JSON. Exit 0 ok, 1 bad input or failed check.

use base_wallet::*;
use serde_json::json;

fn arg(args: &[String], name: &str) -> Option<String> {
    args.iter()
        .position(|a| a == name)
        .and_then(|i| args.get(i + 1).cloned())
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let out = match args.first().map(String::as_str) {
        Some("derive") => (|| {
            let account = arg(&args, "--account").ok_or("--account missing")?;
            let path = arg(&args, "--path").unwrap_or_else(|| agent_core::base::BASE_PATH.into());
            let (root, mock) = if args.iter().any(|a| a == "--mock") {
                (MockMpc::default().public_key(), true)
            } else {
                (parse_near_secp_pk(&arg(&args, "--mpc-pk").ok_or("--mpc-pk or --mock missing")?)?, false)
            };
            if !mock && !account.ends_with(".testnet") {
                return Err("testnet accounts only".to_string());
            }
            let child = derive(&root, &account, &path);
            Ok(json!({"schema": "base-derive.v1", "account": account, "path": path,
                      "mpc_public_key": near_secp_pk(&root), "mock_signer": mock,
                      "public_key": near_secp_pk(&child), "address": address(&child),
                      "chain_id": agent_core::base::BASE_CHAIN_ID}))
        })(),
        Some("check-auth") => (|| {
            let f = args.get(1).ok_or("file missing")?;
            let v: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(f).map_err(|e| e.to_string())?)
                .map_err(|e| e.to_string())?;
            let e = if v["auth"].is_object() { v["auth"].clone() } else { v };
            let who = check_auth(&e)?;
            Ok(json!({"ok": true, "signer": who, "relay_to": agent_core::base::BASE_USDC, "calldata": e["calldata"]}))
        })(),
        _ => Err("usage: base-wallet derive (--mpc-pk PK | --mock) --account ID [--path P] | check-auth FILE".into()),
    };
    match out {
        Ok(v) => println!("{}", serde_json::to_string_pretty(&v).unwrap()),
        Err(e) => {
            eprintln!("base-wallet: {e}");
            std::process::exit(1);
        }
    }
}
