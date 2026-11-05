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

/// Read-only exports the reproducible cargo-near build adds on its own: the
/// embedded ABI and NEP-330 source metadata. Not in the `#[near] impl`.
pub const BUILD_EXPORTS: [&str; 2] = ["__contract_abi", "contract_source_metadata"];

/// Every public method of the published agent contract. A deployed contract
/// exposing anything else is not the published code. A test keeps this list
/// equal to the `#[near] impl` in `contracts/agent/src/lib.rs`.
pub const PUBLISHED_METHODS: [&str; 59] = [
    "add_signer",
    "cancel_listing",
    "cancel_offer",
    "claim",
    "close_share",
    "finalize_earnout",
    "ft_on_transfer",
    "get_base_wallet",
    "get_claimable",
    "get_earnings",
    "get_earnings_days",
    "get_holdings",
    "get_limits",
    "get_listing",
    "get_payer",
    "get_sales",
    "get_shares",
    "get_signers",
    "get_status",
    "heartbeat",
    "known_keys",
    "known_keys_hash",
    "list_for_sale",
    "near_withdrawable",
    "new",
    "nft_approve",
    "nft_is_approved",
    "nft_metadata",
    "nft_resolve_transfer",
    "nft_revoke",
    "nft_revoke_all",
    "nft_supply_for_owner",
    "nft_token",
    "nft_tokens",
    "nft_tokens_for_owner",
    "nft_total_supply",
    "nft_transfer",
    "nft_transfer_call",
    "offer_share",
    "on_base_signed",
    "on_handover",
    "on_keys_deleted",
    "on_runner_key",
    "on_transfer_out",
    "pay",
    "pay_base",
    "reclaim_payer_row",
    "recover_keys",
    "recover_sale",
    "remove_signer",
    "set_limits",
    "set_runner_key",
    "storage_balance_bounds",
    "storage_balance_of",
    "storage_deposit",
    "storage_unregister",
    "storage_withdraw",
    "withdraw",
    "withdraw_near",
];

/// Registry admin and payment token of the published release (A26). The
/// contract compiles them in; a deployment showing other values is not the
/// published release, whatever its code hash says.
pub const PUBLISHED_ADMIN: &str = "admin.testnet";
pub const PUBLISHED_TOKEN: &str = "xusd.testnet";

/// The least earn-out protection a buyer accepts by default (A32): 30% of the
/// price in escrow, the seller's floor at most 50% of it, a 30-day window.
/// The same minimum the published contract enforces. A buyer may ask for
/// more; a demo may shorten the window but must say so.
pub const PUBLISHED_MIN_TERMS: MinTerms = MinTerms {
    min_earnout_bps: 3_000,
    max_floor_bps: 5_000,
    min_window_secs: 30 * 86_400,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct MinTerms {
    pub min_earnout_bps: u16,
    pub max_floor_bps: u16,
    pub min_window_secs: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct TermsView {
    pub earnout_bps: u16,
    pub share_bps: u16,
    pub floor_bps: u16,
    pub window_secs: u64,
}

/// The listing as read in the same view call as the price (`get_listing`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ListingView {
    pub listing_version: u64,
    /// Base units, as a string.
    pub price: String,
    pub terms: TermsView,
    /// Share of every future payment owed to investors (SH7); the buyer
    /// inherits it.
    #[serde(default)]
    pub shares_bps: u16,
    /// The open share slices as `get_listing` shows them.
    #[serde(default)]
    pub shares: Vec<serde_json::Value>,
}

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
    /// Registry admin of the release (default `PUBLISHED_ADMIN`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub admin: Option<String>,
    /// Payment token of the release (default `PUBLISHED_TOKEN`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub token: Option<String>,
    /// The price the buyer was quoted (base units), from the same read as
    /// `listing_version`. Absent = not checked.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub price: Option<String>,
    /// Least terms the buyer accepts (default `PUBLISHED_MIN_TERMS`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub min_terms: Option<MinTerms>,
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
    /// `get_status.admin` / `get_status.token`. Missing = cannot check = block.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub admin: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub token: Option<String>,
    /// `get_status.paid_to_total`: payout destinations tracked for the
    /// round-trip flag. Missing = cannot check = block (A36).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub paid_to_total: Option<u64>,
    /// `get_status.near_storage.solvent`: the NEAR balance covers storage
    /// staking, the bounded signature window and the reserve. Missing =
    /// cannot check = block (A46).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub near_storage_solvent: Option<bool>,
    /// `get_base_wallet` plus the MPC signer's root key, read by the buyer
    /// (A51). Missing = cannot check = block.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub base_wallet: Option<BaseWalletView>,
}

/// The agent's Base wallet as the contract reports it, and the MPC root key
/// the buyer read from the signer contract (`v1.signer-prod.testnet`
/// `public_key`), not from the agent (A51).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BaseWalletView {
    /// `get_base_wallet.address`; `None` = the agent has no Base wallet.
    pub address: Option<String>,
    /// `get_base_wallet.path`.
    pub path: String,
    /// `secp256k1:<base58>`; read by the scanner from the MPC contract.
    pub mpc_public_key: Option<String>,
}

/// The derivation path of the agent's Base wallet (feature 5).
pub const BASE_PATH: &str = agent_core::base::BASE_PATH;

/// The Base address the MPC signer controls for (`agent`, `path`) under
/// `mpc_public_key`, or why it cannot be derived.
pub fn derive_base_address(
    mpc_public_key: &str,
    agent: &str,
    path: &str,
) -> Result<String, String> {
    let root = base_wallet::parse_near_secp_pk(mpc_public_key)?;
    Ok(base_wallet::derive_address(&root, agent, path))
}

/// Payout destinations the agent tracks across all owners, and the quota
/// each new owner gets (same as the contract's `MAX_PAID_TO_TOTAL` and
/// `MAX_PAID_TO`). Above `TOTAL - QUOTA` the buyer would get less (A36).
pub const PAID_TO_TOTAL: u64 = 3_000;
pub const PAID_TO_QUOTA: u64 = 1_000;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ScanInput {
    pub schema: String,
    pub agent: String,
    /// Scan time, ns since the epoch.
    pub now_ns: u64,
    pub expected: Expected,
    pub chain: Chain,
    pub contract: ContractView,
    /// `get_listing` at scan time. Missing = cannot check the terms = block.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub listing: Option<ListingView>,
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

/// Keys as two providers must agree on them: the key and what it may do.
/// The allowance is left out (review 5, M2): it drops on every call the
/// runner makes, so providers a block apart see different numbers.
fn key_set(keys: &[AccessKey]) -> BTreeSet<(String, String)> {
    keys.iter()
        .map(|k| {
            let p = match &k.permission {
                Permission::FunctionCall {
                    receiver_id,
                    method_names,
                    ..
                } => Permission::FunctionCall {
                    receiver_id: receiver_id.clone(),
                    method_names: method_names.clone(),
                    allowance: None,
                },
                full => full.clone(),
            };
            (
                k.public_key.clone(),
                serde_json::to_string(&p).unwrap_or_default(),
            )
        })
        .collect()
}

/// Codes about the sale listing only: an investor buying a share slice of
/// an agent that is not for sale does not need them.
pub const LISTING_ONLY: [&str; 5] = [
    "not_listed",
    "listing_missing",
    "listing_version_mismatch",
    "price_changed",
    "terms_below_minimum",
];

/// The investor's scan before `buy_share`: every key, code and contract
/// rule of the sale scan, without the listing rules (T8). No buy arguments.
pub fn scan_investor(i: &ScanInput) -> Report {
    let mut r = scan(i);
    r.findings
        .retain(|f| !LISTING_ONLY.contains(&f.code.as_str()));
    r.buy_args = None;
    if r.verdict == "blocked" && !r.findings.iter().any(|f| f.severity == "block") {
        r.verdict = "clean".into();
    }
    r
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
    let published: BTreeSet<&str> = PUBLISHED_METHODS.into_iter().chain(BUILD_EXPORTS).collect();
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

    // A26: admin and token must be the published release's.
    let want_admin = i.expected.admin.as_deref().unwrap_or(PUBLISHED_ADMIN);
    let want_token = i.expected.token.as_deref().unwrap_or(PUBLISHED_TOKEN);
    for (what, want, got) in [
        ("admin", want_admin, i.contract.admin.as_deref()),
        ("token", want_token, i.contract.token.as_deref()),
    ] {
        if got != Some(want) {
            f.push(block(
                "registry_mismatch",
                format!(
                    "{what} is {}, the published release uses {want}",
                    got.unwrap_or("not shown")
                ),
            ));
        }
    }

    // A36: the round-trip table the buyer inherits leaves them a full quota.
    match i.contract.paid_to_total {
        Some(n) if n <= PAID_TO_TOTAL - PAID_TO_QUOTA => {}
        got => f.push(block(
            "paid_to_bloated",
            match got {
                Some(n) => format!(
                    "{n} payout destinations tracked; the buyer could add only {} of {PAID_TO_QUOTA}",
                    PAID_TO_TOTAL.saturating_sub(n)
                ),
                None => "paid_to_total not shown".into(),
            },
        )),
    }

    // A46: an agent whose NEAR balance cannot cover its storage stops
    // writing state (every payment, buy and payout fails).
    if i.contract.near_storage_solvent != Some(true) {
        f.push(block(
            "near_storage_insolvent",
            match i.contract.near_storage_solvent {
                Some(_) => {
                    "the NEAR balance does not cover storage staking plus the reserve".into()
                }
                None => "near_storage.solvent not shown".into(),
            },
        ));
    }

    // A51: the Base address is an init argument, so the code hash does not
    // cover it. Derive it from the MPC root key and compare: a seller could
    // deploy the published wasm with an address they control.
    match &i.contract.base_wallet {
        None => f.push(block(
            "base_wallet_unchecked",
            "get_base_wallet not read".into(),
        )),
        Some(BaseWalletView { address: None, .. }) => {}
        Some(BaseWalletView {
            address: Some(addr),
            path,
            mpc_public_key,
        }) => {
            if path != BASE_PATH {
                f.push(block(
                    "base_address_mismatch",
                    format!("Base wallet path {path}, expected {BASE_PATH}"),
                ));
            } else {
                match mpc_public_key
                    .as_deref()
                    .ok_or_else(|| "MPC public key not read".to_string())
                    .and_then(|pk| derive_base_address(pk, &i.agent, path))
                {
                    Err(e) => f.push(block("base_wallet_unchecked", e)),
                    Ok(want) if want.eq_ignore_ascii_case(addr) => {}
                    Ok(want) => f.push(block(
                        "base_address_mismatch",
                        format!(
                            "get_base_wallet.address {addr} is not the MPC address {want} of ({}, {path})",
                            i.agent
                        ),
                    )),
                }
            }
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
    // A32: the terms the buyer will get, from the listing itself.
    match &i.listing {
        None => f.push(block(
            "listing_missing",
            "no listing (get_listing) to check the price and earn-out terms against".into(),
        )),
        Some(l) => {
            if l.listing_version != i.expected.listing_version
                && i.contract.listing_version == i.expected.listing_version
            {
                f.push(block(
                    "listing_version_mismatch",
                    format!(
                        "listing is at version {}, the buyer was quoted {}",
                        l.listing_version, i.expected.listing_version
                    ),
                ));
            }
            if let Some(p) = &i.expected.price {
                if p != &l.price {
                    f.push(block(
                        "price_changed",
                        format!("listing price is {}, the buyer was quoted {p}", l.price),
                    ));
                }
            }
            let min = i.expected.min_terms.unwrap_or(PUBLISHED_MIN_TERMS);
            let t = &l.terms;
            let mut low = Vec::new();
            if t.earnout_bps < min.min_earnout_bps {
                low.push(format!(
                    "escrow {}% < {}%",
                    t.earnout_bps / 100,
                    min.min_earnout_bps / 100
                ));
            }
            if t.floor_bps > min.max_floor_bps {
                low.push(format!(
                    "seller floor {}% of escrow > {}%",
                    t.floor_bps / 100,
                    min.max_floor_bps / 100
                ));
            }
            if t.window_secs < min.min_window_secs {
                low.push(format!(
                    "window {} s < {} s",
                    t.window_secs, min.min_window_secs
                ));
            }
            if !low.is_empty() {
                f.push(block(
                    "terms_below_minimum",
                    format!(
                        "earn-out terms below the buyer's minimum: {}",
                        low.join(", ")
                    ),
                ));
            }
        }
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
    if let Some(l) = i.listing.as_ref().filter(|l| l.shares_bps > 0) {
        let holders: usize = l
            .shares
            .iter()
            .map(|o| o["holders"].as_array().map_or(0, Vec::len))
            .sum();
        // Review 2 M1: say what the buyer cannot do while the slices run.
        let last_end = l
            .shares
            .iter()
            .filter_map(|o| {
                o["end"]
                    .as_str()
                    .and_then(|e| e.parse::<u64>().ok())
                    .or_else(|| o["end"].as_u64())
            })
            .max()
            .unwrap_or(0);
        let days = last_end.saturating_sub(i.now_ns) / 86_400_000_000_000;
        f.push(Finding {
            code: "shares_disclosed".into(),
            severity: "warn".into(),
            detail: format!(
                "{}% of every future payment goes to investors ({} slice(s), {} holder(s)); the buyer inherits this, and cannot raise the price floor, for up to {} more day(s)",
                l.shares_bps as f64 / 100.0,
                l.shares.len(),
                holders,
                days
            ),
        });
    }
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
                "listing_version": i.expected.listing_version,
                "known_keys_hash": i.contract.known_keys_hash,
                "price": i.listing.as_ref().map(|l| l.price.clone()),
            })
        }),
        findings: f,
    }
}
