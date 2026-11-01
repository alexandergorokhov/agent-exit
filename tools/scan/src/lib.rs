//! Client-side backdoor scan (feature 3).
//!
//! Input (`scan-input.v1`): what a buyer reads before paying. Offline it comes
//! from a JSON fixture; online it comes from RPC (`view_access_key_list` from
//! two providers at `finality: final`, `view_account.code_hash`, the
//! contract's `get_status` view and the method list of the deployed code).
//!
//! Output (`scan-report.v1`): `clean`, `blocked` or `inconclusive`, with one
//! finding per problem. Any `block` finding blocks the sale; the SDK and MCP
//! `buy` refuse unless the verdict is `clean` and then pass the scanned
//! `listing_version` and `known_keys_hash` to the contract, which refuses the
//! buy if either changed (fail-closed in the buy path).

pub mod corpus;

use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;

pub const INPUT_SCHEMA: &str = "scan-input.v1";
pub const REPORT_SCHEMA: &str = "scan-report.v1";

/// Methods the runner's function-call key must be limited to (same as the
/// contract's `RUNNER_METHODS`).
pub const RUNNER_METHODS: [&str; 4] = ["heartbeat", "accept_job", "pay", "pay_base"];

/// Every public method of the published agent contract. A deployed contract
/// exposing anything else is not the published code. A test keeps this list
/// equal to the `#[near] impl` in `contracts/agent/src/lib.rs`.
pub const PUBLISHED_METHODS: [&str; 37] = [
    "new",
    "add_signer",
    "remove_signer",
    "get_signers",
    "set_runner_key",
    "on_runner_key",
    "list_for_sale",
    "cancel_listing",
    "ft_on_transfer",
    "on_handover",
    "on_transfer_out",
    "claim",
    "withdraw",
    "withdraw_near",
    "finalize_earnout",
    "heartbeat",
    "get_earnings",
    "known_keys",
    "known_keys_hash",
    "get_listing",
    "get_sales",
    "get_claimable",
    "get_status",
    "near_withdrawable",
    "nft_metadata",
    "nft_token",
    "nft_total_supply",
    "nft_tokens",
    "nft_supply_for_owner",
    "nft_tokens_for_owner",
    "nft_transfer",
    "nft_transfer_call",
    "nft_resolve_transfer",
    "nft_approve",
    "nft_revoke",
    "nft_revoke_all",
    "nft_is_approved",
];

/// Method names that let someone change the code or the keys: an upgrade
/// path or a self-deploy. Their presence blocks even if the hash looks right.
pub const UPGRADE_WORDS: [&str; 9] = [
    "upgrade",
    "deploy",
    "migrate",
    "set_code",
    "update_contract",
    "add_key",
    "add_full_access_key",
    "delete_account",
    "stage_code",
];

/// What the scan cannot see, by design (printed by `make attack`, kept in
/// docs/NOT-CAUGHT.md).
pub const NOT_CAUGHT: [(&str, &str, &str); 6] = [
    ("N1", "Copies of off-chain code, prompts, model weights", "Not on-chain; the earn-out prices it in"),
    ("N2", "Outside API keys and accounts (LLM keys, market tokens, customer contacts)", "Not on-chain"),
    ("N3", "Sybil payers on aged, fresh or exchange-funded accounts", "Look like customers on-chain; the meter only flags self, sub-accounts and round trips; earn-out share <= 50% makes wash-paying cost money"),
    ("N4", "A hidden full-access key used directly by a buyer who skipped the scan", "The contract cannot list its own keys; the SDK and MCP never skip the scan; a hidden key calling the contract freezes it (A23)"),
    ("N5", "Customers moving off-platform after the sale", "Business risk, not a key"),
    ("N6", "A compromised MPC network, NEAR AI signer or both RPC providers", "Trust assumptions"),
];

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum Permission {
    /// `"FullAccess"`
    Full(String),
    FunctionCall {
        receiver_id: String,
        method_names: Vec<String>,
        #[serde(default)]
        allowance: Option<String>,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AccessKey {
    pub public_key: String,
    pub permission: Permission,
}

impl AccessKey {
    pub fn is_full(&self) -> bool {
        // Any non-function-call permission is treated as full access.
        matches!(&self.permission, Permission::Full(_))
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Expected {
    /// sha256 of the reproducible build (contracts/res/SHA256SUMS) or the
    /// global-contract hash.
    pub code_hash: String,
    /// The listing version the buyer was quoted.
    pub listing_version: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Chain {
    pub code_hash: String,
    pub methods: Vec<String>,
    /// Key list from provider 1.
    pub keys: Vec<AccessKey>,
    /// Key list from provider 2 (absent offline = same as provider 1).
    #[serde(default)]
    pub keys_second_provider: Option<Vec<AccessKey>>,
    #[serde(default)]
    pub subaccounts: Vec<String>,
}

/// The contract's own view (`get_status`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ContractView {
    pub status: String,
    pub owner: String,
    pub listing_version: u64,
    pub known_keys: Vec<String>,
    pub known_keys_hash: String,
    pub open_approvals: u64,
    /// Latest valid-before (ns) of a signed Base authorization; 0 = none.
    #[serde(default)]
    pub open_auth_until: u64,
    #[serde(default)]
    pub frozen: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ScanInput {
    pub schema: String,
    pub agent: String,
    /// Scan time, ns since the epoch.
    pub now_ns: u64,
    pub expected: Expected,
    pub chain: Chain,
    pub contract: ContractView,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Finding {
    pub code: String,
    /// `block` or `warn`.
    pub severity: String,
    pub detail: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Report {
    pub schema: String,
    pub agent: String,
    /// `clean` | `blocked` | `inconclusive`
    pub verdict: String,
    pub findings: Vec<Finding>,
    /// What to pass to the contract's buy msg, only when clean.
    pub buy_args: Option<serde_json::Value>,
}

impl Report {
    pub fn codes(&self) -> Vec<&str> {
        self.findings.iter().map(|f| f.code.as_str()).collect()
    }
    pub fn blocks(&self) -> bool {
        self.verdict != "clean"
    }
}

fn block(code: &str, detail: String) -> Finding {
    Finding {
        code: code.into(),
        severity: "block".into(),
        detail,
    }
}

fn key_set(keys: &[AccessKey]) -> BTreeSet<(String, String)> {
    keys.iter()
        .map(|k| {
            (
                k.public_key.clone(),
                serde_json::to_string(&k.permission).unwrap_or_default(),
            )
        })
        .collect()
}

/// Run every rule. Pure: same input, same report.
pub fn scan(i: &ScanInput) -> Report {
    let mut f: Vec<Finding> = Vec::new();
    let mut inconclusive = false;

    if i.schema != INPUT_SCHEMA {
        f.push(block(
            "bad_input_schema",
            format!("expected {INPUT_SCHEMA}, got {}", i.schema),
        ));
    }

    // E31: two RPC providers must agree on the key list.
    if let Some(second) = &i.chain.keys_second_provider {
        if key_set(second) != key_set(&i.chain.keys) {
            inconclusive = true;
            f.push(block(
                "rpc_disagree",
                "the two RPC providers returned different key lists".into(),
            ));
        }
    }

    // Keys on chain vs the keys the contract says it added.
    let known: BTreeSet<&str> = i.contract.known_keys.iter().map(String::as_str).collect();
    let mut on_chain: BTreeSet<&str> = BTreeSet::new();
    let all_keys = i
        .chain
        .keys
        .iter()
        .chain(i.chain.keys_second_provider.iter().flatten());
    for k in all_keys {
        if !on_chain.insert(k.public_key.as_str()) {
            continue;
        }
        if k.is_full() {
            f.push(block(
                "full_access_key_present",
                format!("{} has full access to the agent account", k.public_key),
            ));
            continue;
        }
        if !known.contains(k.public_key.as_str()) {
            f.push(block(
                "unknown_key",
                format!(
                    "{} is on the account but the contract did not add it",
                    k.public_key
                ),
            ));
            continue;
        }
        if let Permission::FunctionCall {
            receiver_id,
            method_names,
            ..
        } = &k.permission
        {
            let want: BTreeSet<&str> = RUNNER_METHODS.into_iter().collect();
            let got: BTreeSet<&str> = method_names.iter().map(String::as_str).collect();
            if receiver_id != &i.agent || got != want {
                f.push(block(
                    "runner_key_scope",
                    format!(
                        "{} may call {receiver_id} {:?}; a runner key must call only the agent's runner methods",
                        k.public_key, method_names
                    ),
                ));
            }
        }
    }
    for k in &known {
        if !on_chain.contains(k) {
            f.push(block(
                "known_key_missing",
                format!("the contract lists {k} but the account does not have it"),
            ));
        }
    }
    let recomputed = agent_core::keys_hash(i.contract.known_keys.iter());
    if recomputed != i.contract.known_keys_hash {
        f.push(block(
            "known_keys_hash_mismatch",
            "known_keys_hash does not match the listed keys".into(),
        ));
    }

    // Code.
    if i.chain.code_hash != i.expected.code_hash {
        f.push(block(
            "code_hash_mismatch",
            format!(
                "deployed {} != published {}",
                i.chain.code_hash, i.expected.code_hash
            ),
        ));
    }
    let published: BTreeSet<&str> = PUBLISHED_METHODS.into_iter().collect();
    for m in &i.chain.methods {
        let lower = m.to_ascii_lowercase();
        if UPGRADE_WORDS.iter().any(|w| lower.contains(w)) {
            f.push(block(
                "upgrade_method",
                format!("method `{m}` can change the code or keys"),
            ));
        } else if !published.contains(m.as_str()) {
            f.push(block(
                "unexpected_method",
                format!("method `{m}` is not in the published contract"),
            ));
        }
    }

    // Contract state.
    if i.contract.frozen {
        f.push(block(
            "frozen",
            "the agent is frozen (a backdoor was proven)".into(),
        ));
    }
    if i.contract.status != "Listed" {
        f.push(block(
            "not_listed",
            format!("status is {}", i.contract.status),
        ));
    }
    if i.contract.listing_version != i.expected.listing_version {
        f.push(block(
            "listing_version_mismatch",
            format!(
                "listing is at version {}, the buyer was quoted {}",
                i.contract.listing_version, i.expected.listing_version
            ),
        ));
    }
    if i.contract.open_approvals > 0 {
        f.push(block(
            "open_approval",
            format!("{} open approval(s) on the deed", i.contract.open_approvals),
        ));
    }
    if i.contract.open_auth_until > i.now_ns {
        f.push(block(
            "open_base_auth",
            "a signed Base authorization is still valid".into(),
        ));
    }

    // Warnings (do not block).
    for s in &i.chain.subaccounts {
        f.push(Finding {
            code: "subaccount_does_not_move".into(),
            severity: "warn".into(),
            detail: format!("{s} is a sub-account; it does not move with the sale"),
        });
    }

    let blocked = f.iter().any(|x| x.severity == "block");
    let verdict = if inconclusive {
        "inconclusive"
    } else if blocked {
        "blocked"
    } else {
        "clean"
    };
    Report {
        schema: REPORT_SCHEMA.into(),
        agent: i.agent.clone(),
        verdict: verdict.into(),
        buy_args: (verdict == "clean").then(|| {
            serde_json::json!({
                "listing_version": i.contract.listing_version,
                "known_keys_hash": i.contract.known_keys_hash,
            })
        }),
        findings: f,
    }
}
