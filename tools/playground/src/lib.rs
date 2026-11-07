//! Agent Exit playground: the demo page runs this in the browser (wasm).
//!
//! Nothing here re-implements the rules. Every answer comes from
//! `agent_core::Machine` (the contract's state machine, the same code the
//! NEAR contract wraps) and `scan::scan` (the buyer's backdoor scan). This
//! crate only plays the parts the chain plays around them: the account's key
//! list, the key batch of a handover, the token transfers, and the clock.
//!
//! Interface: one JSON action in, one JSON answer out (`handle`). In wasm,
//! `pg_alloc` / `pg_call` / `pg_out` pass the strings through linear memory,
//! so the page needs no bindings library.

use agent_core::{
    BuyResult, Machine, PayResult, RunnerCheck, Terms, TransferKind, Ts, DAY_NS, USD,
};
use scan::{AccessKey, Permission, Report, ScanInput};
use serde::Deserialize;
use serde_json::{json, Value};
use std::cell::RefCell;

pub const AGENT: &str = "translator.testnet";
pub const SELLER: &str = "alice.testnet";
pub const BUYER: &str = "bob.testnet";
pub const INVESTOR: &str = "dave.testnet";
/// The runner key in the recorded world (fixtures/mcp/world.json).
pub const SELLER_KEY: &str = "ed25519:4vJ9JU1bJJE96FWSJKvHsmmFADCg4gpZQff4P3bkLKi";
pub const BUYER_KEY: &str = "ed25519:8NZ8qJvTgpBmhSUt6vAaRkPZAG3JVVwrE3D25Lczz6Z5";
pub const HIDDEN_KEY: &str = "ed25519:H1ddenBackdoorKeyXz4fS7sUuVJgq2y3QW9rNmLpTcAa";
const FEE_ACCOUNT: &str = "fees.testnet";
const T0: Ts = 1_768_089_600 * 1_000_000_000; // the recorded world's scan time
const WORLD: &str = include_str!("../../../fixtures/mcp/world.json");

#[derive(Debug, Clone)]
pub struct World {
    pub m: Machine,
    /// The account's access keys as the node reports them.
    pub keys: Vec<AccessKey>,
    pub upgrade_method: bool,
    /// The seller redeployed through a hidden full-access key (the contract is gone).
    pub taken_over: bool,
    pub now: Ts,
    payer_n: u32,
    pub scan: Option<Report>,
    log: Vec<Value>,
    template: ScanInput,
    /// The last sealed delivery (ephemeral key, ciphertext).
    cipher: Option<([u8; 32], Vec<u8>)>,
}

#[derive(Deserialize)]
#[serde(tag = "op", rename_all = "snake_case")]
enum Action {
    Reset,
    View,
    PayCustomer,
    PayVerified,
    PaySelf,
    PaySub,
    PlantKey,
    RemoveKey,
    ToggleUpgrade,
    List {
        price: Option<u64>,
        /// The whole price held, paid to the seller only from revenue (PR1).
        pay_from_revenue: Option<bool>,
    },
    /// The owner pays 200 from a fresh wallet the meter cannot link to them.
    PayFresh,
    Cancel,
    Scan,
    Buy {
        skip_scan: Option<bool>,
    },
    Withdraw,
    Relist,
    SetKey,
    OldKey,
    HiddenKey,
    HiddenRedeploy,
    Wait {
        days: Option<u64>,
    },
    /// Owner offers 20% of every future payment for a year at 500.
    OfferShare,
    /// dave buys the whole open slice, with the terms he saw.
    BuyShare,
    /// dave claims what the agent paid him.
    ClaimShare,
    /// A year passes; anyone closes the slice; dave gets his escrow back.
    EndShare,
    /// alice commits to the agent's sealed config (prompt, tools, keys to rotate).
    CommitConfig,
    /// alice delivers the committed config, sealed to bob's one-time key.
    DeliverConfig,
    /// alice delivers a different prompt of the same length.
    DeliverFake,
    /// bob opens what alice delivered and checks it against the commitment.
    OpenConfig,
    /// bob reveals his one-time key; the contract decrypts and compares.
    DisputeConfig,
    /// Three days pass with no delivery; bob takes the escrow back.
    ClaimUndelivered,
}

/// The sealed config in the playground (sealed.rs): what alice sells with the agent.
const BUNDLE: &[u8] = br#"{"schema":"agent-config.v1","system_prompt":"You translate EN<->ES. Keep names and numbers exact.","model":"deepseek-v3","tools":["glossary"],"rotate_after_sale":["NEARAI_API_KEY"]}"#;
const BOB_DELIVERY_SK: [u8; 32] = [42u8; 32];
const ALICE_EPH_SK: [u8; 32] = [17u8; 32];

fn usd(a: u128) -> String {
    let whole = a / USD;
    let cents = (a % USD) / (USD / 100);
    let mut s = whole.to_string();
    let mut i = s.len() as isize - 3;
    while i > 0 {
        s.insert(i as usize, ',');
        i -= 3;
    }
    format!("{s}.{cents:02}")
}

fn runner_key(pk: &str) -> AccessKey {
    AccessKey {
        public_key: pk.into(),
        permission: Permission::FunctionCall {
            receiver_id: AGENT.into(),
            method_names: ["heartbeat", "accept_job", "pay", "pay_base"]
                .map(String::from)
                .to_vec(),
            allowance: Some("5000000000000000000000000".into()),
        },
    }
}

impl Default for World {
    fn default() -> Self {
        Self::new()
    }
}

impl World {
    pub fn new() -> Self {
        let w: Value = serde_json::from_str(WORLD).expect("fixtures/mcp/world.json");
        let template: ScanInput =
            serde_json::from_value(w["agents"][AGENT]["scan_input"].clone()).expect("scan_input");
        let mut m = Machine::new(AGENT, SELLER);
        // The contract's 1% protocol fee, credited to the fee account.
        m.fee_to = FEE_ACCOUNT.into();
        let old = m
            .set_runner_key(SELLER, SELLER_KEY, T0)
            .expect("runner key");
        m.runner_key_done(m.key_batch, &[], &old, SELLER_KEY, true);
        let mut w = World {
            m,
            keys: vec![runner_key(SELLER_KEY)],
            upgrade_method: false,
            taken_over: false,
            now: T0,
            payer_n: 0,
            scan: None,
            cipher: None,
            log: Vec::new(),
            template,
        };
        // A small honest history, so the meter has something to show.
        for i in 0..3 {
            w.pay(&format!("customer-{}.testnet", i + 1), 40 * USD, i == 0);
        }
        w.log.clear();
        w
    }

    fn note(&mut self, who: &str, ok: bool, text: String) -> Value {
        let e = json!({"who": who, "ok": ok, "text": text});
        self.log.push(e.clone());
        if self.log.len() > 40 {
            self.log.remove(0);
        }
        e
    }

    fn pay(&mut self, payer: &str, amount: u128, verified: bool) -> String {
        self.now += 6 * 3_600 * 1_000_000_000;
        match self.m.pay(payer, amount, verified, self.now) {
            PayResult::Refund(why) => format!("refunded: {why}"),
            PayResult::Recorded {
                flags,
                counted,
                verified,
                released,
                to_investors,
                share_released,
                ..
            } => {
                let mut s = if counted {
                    if verified {
                        "counted, Verified (NEAR AI signature; signed with a test key here, the live NEAR AI check is gate G6)".to_string()
                    } else {
                        "counted as real revenue".to_string()
                    }
                } else if flags.self_pay {
                    "FLAGGED: the owner paid their own agent (not counted)".to_string()
                } else if flags.seller_subaccount {
                    "FLAGGED: paid from the owner's sub-account (not counted)".to_string()
                } else {
                    "FLAGGED: round trip (not counted)".to_string()
                };
                for (_, seller, r) in released {
                    s.push_str(&format!("; {} of the escrow released to {seller}", usd(r)));
                }
                if to_investors > 0 {
                    s.push_str(&format!(
                        "; {} to investors, paid inside the same call",
                        usd(to_investors)
                    ));
                }
                for (seller, r) in share_released {
                    s.push_str(&format!(
                        " ({} of share escrow released to {seller})",
                        usd(r)
                    ));
                }
                s
            }
        }
    }

    fn scan_input(&self) -> ScanInput {
        let mut i = self.template.clone();
        i.now_ns = self.now;
        let version = self
            .m
            .listing
            .as_ref()
            .map(|l| l.version)
            .unwrap_or(self.m.listing_version);
        i.expected.listing_version = version;
        i.expected.price = None;
        if self.upgrade_method {
            i.chain.methods.push("upgrade".into());
        }
        i.chain.keys = self.keys.clone();
        i.chain.keys_second_provider = Some(self.keys.clone());
        i.contract.status = format!("{:?}", self.m.status);
        i.contract.owner = self.m.owner.clone();
        i.contract.listing_version = self.m.listing_version;
        i.contract.known_keys = self.m.known_keys.iter().cloned().collect();
        i.contract.known_keys_hash = self.m.known_keys_hash();
        i.contract.frozen = self.m.frozen;
        i.contract.paid_to_total = Some(self.m.paid_to_total());
        i.listing = self.m.listing.as_ref().map(|l| scan::ListingView {
            listing_version: l.version,
            price: l.price.to_string(),
            terms: scan::TermsView {
                earnout_bps: l.terms.earnout_bps,
                share_bps: l.terms.share_bps,
                floor_bps: l.terms.floor_bps,
                window_secs: l.terms.window_secs,
            },
            shares_bps: self.m.shares_bps(),
            shares: self.m.offers.iter().map(|o| serde_json::json!({
                "offer_id": o.id, "bps": o.bps,
                "holders": o.holders.iter().map(|h| serde_json::json!({"account": h.account, "bps": h.bps})).collect::<Vec<_>>(),
            })).collect(),
        });
        i
    }

    fn act(&mut self, a: Action) -> Value {
        let s = SELLER;
        match a {
            Action::Reset => {
                *self = World::new();
                self.note(
                    "page",
                    true,
                    "New world: alice owns translator, 3 real customers paid it.".into(),
                )
            }
            Action::View => json!(null),
            Action::PayCustomer | Action::PayVerified => {
                self.payer_n += 1;
                let payer = format!("client-{}.testnet", self.payer_n);
                let r = self.pay(&payer, 25 * USD, matches!(a, Action::PayVerified));
                self.note(&payer, true, format!("pays 25.00: {r}"))
            }
            Action::PaySelf => {
                let owner = self.m.owner.clone();
                let r = self.pay(&owner, 200 * USD, false);
                self.note(&owner, true, format!("pays own agent 200.00: {r}"))
            }
            Action::PayFresh => {
                let Some(seller) = self.m.sales.last().map(|x| x.seller.clone()) else {
                    let r = self.pay("fresh-wallet-7.testnet", 200 * USD, false);
                    return self.note(
                        "fresh-wallet-7.testnet",
                        true,
                        format!("alice's fresh wallet pays 200.00: {r}. Before a sale the money comes back to alice as revenue, so this costs her nothing: that is why the buyer's card weighs payers first seen in the last 7 days at half"),
                    );
                };
                let before = self.m.claimable_of(&seller);
                let r = self.pay("fresh-wallet-7.testnet", 200 * USD, false);
                let got = self.m.claimable_of(&seller) - before;
                self.note(
                    "fresh-wallet-7.testnet",
                    true,
                    format!(
                        "{seller}'s fresh wallet pays 200.00: {r}. {seller} paid 200.00 and got {} back: faking revenue cost {}",
                        usd(got),
                        usd(200 * USD - got)
                    ),
                )
            }
            Action::PaySub => {
                let payer = format!("bot.{}", self.m.owner);
                let r = self.pay(&payer, 200 * USD, false);
                self.note(&payer, true, format!("pays 200.00: {r}"))
            }
            Action::PlantKey => {
                if !self.keys.iter().any(|k| k.public_key == HIDDEN_KEY) {
                    self.keys.push(AccessKey {
                        public_key: HIDDEN_KEY.into(),
                        permission: Permission::Full("FullAccess".into()),
                    });
                }
                self.note(s, true, "adds a hidden full-access key to the agent account (the contract never sees it)".into())
            }
            Action::RemoveKey => {
                self.keys.retain(|k| k.public_key != HIDDEN_KEY);
                self.note(s, true, "deletes the hidden key".into())
            }
            Action::ToggleUpgrade => {
                self.upgrade_method = !self.upgrade_method;
                let t = if self.upgrade_method {
                    "deploys a build with an `upgrade` method (a way to swap the code later)"
                } else {
                    "redeploys the clean release build"
                };
                self.note(s, true, t.into())
            }
            Action::List {
                price,
                pay_from_revenue,
            } => {
                let p = price.unwrap_or(1_000) as u128 * USD;
                let terms = if pay_from_revenue.unwrap_or(false) {
                    Terms {
                        earnout_bps: agent_core::MAX_EARNOUT_BPS,
                        floor_bps: 0,
                        ..Terms::default()
                    }
                } else {
                    Terms::default()
                };
                let r = self
                    .m
                    .list(s, p, terms, "fixture-release-hash-v1", self.now);
                self.scan = None;
                match r {
                    Ok(l) => self.note(
                        s,
                        true,
                        format!(
                            "list_for_sale({}) -> Listed, version {}",
                            usd(l.price),
                            l.version
                        ),
                    ),
                    Err(e) => self.note(s, false, format!("list_for_sale -> {e}")),
                }
            }
            Action::Cancel => match self.m.cancel_listing(s) {
                Ok(()) => self.note(s, true, "cancel_listing -> not listed".into()),
                Err(e) => self.note(s, false, format!("cancel_listing -> {e}")),
            },
            Action::Scan => {
                let r = scan::scan(&self.scan_input());
                let codes = r.codes().join(", ");
                let text = if r.blocks() {
                    format!("scan -> {} ({codes})", r.verdict)
                } else {
                    "scan -> clean: safe to buy".into()
                };
                let ok = !r.blocks();
                self.scan = Some(r);
                self.note(BUYER, ok, text)
            }
            Action::Buy { skip_scan } => self.buy(skip_scan.unwrap_or(false)),
            Action::OfferShare => {
                let owner = self.m.owner.clone();
                let end = self.now + 365 * DAY_NS;
                match self.m.offer_share(&owner, 2_000, 500 * USD, end, self.now) {
                    Ok(id) => self.note(
                        &owner,
                        true,
                        format!(
                            "offer_share(20% of every payment for 1 year, 500.00) -> slice {id}"
                        ),
                    ),
                    Err(e) => self.note(&owner, false, format!("offer_share -> {e}")),
                }
            }
            Action::BuyShare => {
                let Some(o) = self.m.offers.iter().find(|o| o.sold_bps < o.bps).cloned() else {
                    return self.note(
                        INVESTOR,
                        false,
                        "buy_share -> nothing on offer (offer a slice first)".into(),
                    );
                };
                let part = o.bps - o.sold_bps;
                let cost = o.cost(part);
                match self.m.buy_share(INVESTOR, cost, o.terms(), part, self.now) {
                    Ok(c) => {
                        let (now_part, esc) = agent_core::shares::split_cost(c);
                        let fee = agent_core::money::protocol_fee(c);
                        self.note(INVESTOR, true, format!(
                            "buy_share({}%) -> paid {}: {} to {} now (after the 1% protocol fee, {}), {} held in escrow until dave is paid that much",
                            part as f64 / 100.0, usd(c), usd(now_part - fee), o.seller, usd(fee), usd(esc)))
                    }
                    Err(e) => self.note(INVESTOR, false, format!("buy_share -> {e}")),
                }
            }
            Action::ClaimShare => match self.m.claim(INVESTOR) {
                Ok(a) => self.note(INVESTOR, true, format!("claim -> {} sent to dave", usd(a))),
                Err(e) => self.note(INVESTOR, false, format!("claim -> {e}")),
            },
            Action::CommitConfig => {
                let c = agent_core::sealed::ConfigCommit::of(BUNDLE);
                match self.m.commit_config(s, Some(c.clone())) {
                    Ok(()) => self.note(s, true, format!(
                        "commit_config(sha256 {}..., {} bytes) -> the next listing sells this exact prompt and tool setup, sealed", &c.hash[..12], c.bytes)),
                    Err(e) => self.note(s, false, format!("commit_config -> {e}")),
                }
            }
            Action::DeliverConfig | Action::DeliverFake => {
                let fake = matches!(a, Action::DeliverFake);
                let mut bundle = BUNDLE.to_vec();
                if fake {
                    // "translate" -> "Translate": a different prompt, same length.
                    bundle[50] = b'X';
                }
                let Some(sale) = self.m.sales.last().cloned() else {
                    return self.note(SELLER, false, "deliver_config -> UnknownSale".into());
                };
                let bob_pk = agent_core::sealed::public_key(&BOB_DELIVERY_SK);
                let (eph, cipher) =
                    agent_core::sealed::seal(&ALICE_EPH_SK, &bob_pk, &bundle).expect("valid key");
                match self.m.deliver_config(
                    SELLER,
                    sale.id,
                    &agent_core::sealed::to_hex(&eph),
                    &cipher,
                    self.now,
                ) {
                    Ok(()) => {
                        self.cipher = Some((eph, cipher));
                        let what = if fake {
                            "a DIFFERENT prompt of the same length"
                        } else {
                            "the committed config"
                        };
                        self.note(SELLER, true, format!("deliver_config -> {} bytes sealed to bob's one-time key ({what}); releases to alice can start", BUNDLE.len()))
                    }
                    Err(e) => self.note(SELLER, false, format!("deliver_config -> {e}")),
                }
            }
            Action::OpenConfig => {
                let Some((eph, cipher)) = self.cipher.clone() else {
                    return self.note(BUYER, false, "open -> nothing delivered yet".into());
                };
                let plain =
                    agent_core::sealed::open(&BOB_DELIVERY_SK, &eph, &cipher).unwrap_or_default();
                let ok = agent_core::sealed::ConfigCommit::of(&plain)
                    == agent_core::sealed::ConfigCommit::of(BUNDLE);
                let text = String::from_utf8_lossy(&plain)
                    .chars()
                    .take(90)
                    .collect::<String>();
                self.note(BUYER, ok, if ok {
                    format!("opens to the committed config (sha256 matches): {text}... Now rotate NEARAI_API_KEY")
                } else {
                    format!("does NOT match the commitment: {text}...")
                })
            }
            Action::DisputeConfig => {
                let Some(sale) = self.m.sales.last().cloned() else {
                    return self.note(BUYER, false, "dispute_config -> UnknownSale".into());
                };
                let cipher = self.cipher.clone().map(|c| c.1).unwrap_or_default();
                self.now += 3_600 * 1_000_000_000;
                match self.m.dispute_config(BUYER, sale.id, &agent_core::sealed::to_hex(&BOB_DELIVERY_SK), &cipher, self.now) {
                    Ok(r) => self.note(BUYER, true, format!(
                        "dispute_config -> the contract decrypted the delivery on-chain: not the committed bytes; {} of escrow back to bob", usd(r))),
                    Err(e) => self.note(BUYER, false, format!("dispute_config -> {e}")),
                }
            }
            Action::ClaimUndelivered => {
                let Some(sale) = self.m.sales.last().cloned() else {
                    return self.note(BUYER, false, "claim_undelivered -> UnknownSale".into());
                };
                self.now += 3 * DAY_NS;
                match self.m.claim_undelivered(sale.id, self.now) {
                    Ok(r) => self.note(
                        BUYER,
                        true,
                        format!(
                            "3 days, no delivery: claim_undelivered -> {} of escrow back to bob",
                            usd(r)
                        ),
                    ),
                    Err(e) => self.note(BUYER, false, format!("claim_undelivered -> {e}")),
                }
            }
            Action::EndShare => {
                let Some(o) = self.m.offers.first().cloned() else {
                    return self.note(INVESTOR, false, "close_share -> no slice".into());
                };
                if self.now < o.end {
                    self.now = o.end;
                }
                match self.m.close_share(o.id, self.now) {
                    Ok(r) => {
                        let back: u128 = r.iter().map(|x| x.1).sum();
                        self.note(INVESTOR, true, format!(
                            "a year later: close_share -> slice ended, {} of unreleased escrow back to dave", usd(back)))
                    }
                    Err(e) => self.note(INVESTOR, false, format!("close_share -> {e}")),
                }
            }
            Action::Withdraw => match self.m.withdraw(s, USD, s) {
                Ok(a) => self.note(s, true, format!("withdraw -> {} sent", usd(a))),
                Err(e) => self.note(s, false, format!("withdraw -> {e}")),
            },
            Action::Relist => match self.m.list(
                s,
                900 * USD,
                Terms::default(),
                "fixture-release-hash-v1",
                self.now,
            ) {
                Ok(_) => self.note(s, true, "list_for_sale -> Listed".into()),
                Err(e) => self.note(s, false, format!("list_for_sale -> {e}")),
            },
            Action::SetKey => {
                match self
                    .m
                    .set_runner_key(s, "ed25519:AliceComesBackKey9xYz", self.now)
                {
                    Ok(_) => self.note(s, true, "set_runner_key -> accepted".into()),
                    Err(e) => self.note(s, false, format!("set_runner_key -> {e}")),
                }
            }
            Action::OldKey => self.sign_with(SELLER_KEY, "her old runner key"),
            Action::HiddenKey => self.sign_with(HIDDEN_KEY, "the hidden key"),
            Action::HiddenRedeploy => {
                if !self.keys.iter().any(|k| k.public_key == HIDDEN_KEY) {
                    return self.note(SELLER, false, "deploys new code with the hidden key -> AccessKeyNotFound: there is no hidden key".into());
                }
                // A full-access key acts on the account, not through the contract:
                // DeployContract / DeleteKey actions never reach contract code.
                self.taken_over = true;
                self.keys.retain(|k| k.public_key == HIDDEN_KEY);
                self.note(SELLER, true, "deploys her own code and deletes bob's key with the hidden key -> accepted by the node. The contract never runs, so it cannot stop this: bob paid and alice has the agent back. Only the scan protects the buyer.".into())
            }
            Action::Wait { days } => {
                self.now += days.unwrap_or(30) * DAY_NS;
                let ids: Vec<u64> = self
                    .m
                    .sales
                    .iter()
                    .filter(|x| !x.finalized && self.now >= x.window_end)
                    .map(|x| x.id)
                    .collect();
                let mut t = format!("{} days pass", days.unwrap_or(30));
                for id in ids {
                    if let Ok(sale) = self.m.finalize_earnout(id, self.now) {
                        t.push_str(&format!(
                            "; earn-out closed: {} released to the seller, {} back to the buyer",
                            usd(sale.released),
                            usd(sale.returned)
                        ));
                    }
                }
                self.note("clock", true, t)
            }
        }
    }

    /// A transaction signed by `pk` calling a runner method on the agent.
    fn sign_with(&mut self, pk: &str, what: &str) -> Value {
        if !self.keys.iter().any(|k| k.public_key == pk) {
            return self.note(SELLER, false, format!("signs with {what} -> AccessKeyNotFound: the node rejects it; it never reaches the contract"));
        }
        match self.m.runner_guard(AGENT, pk) {
            Ok(RunnerCheck::Ok) => self.note(
                SELLER,
                true,
                format!("signs with {what} -> accepted (it is a runner key)"),
            ),
            Ok(RunnerCheck::BackdoorProven { returned }) => {
                let back: u128 = returned.iter().map(|(_, _, a)| a).sum();
                self.note(SELLER, false, format!("signs with {what} -> FROZEN: a key the contract never added was used. The agent is frozen and {} of escrow goes back to the buyer", usd(back)))
            }
            Err(e) => self.note(SELLER, false, format!("signs with {what} -> {e}")),
        }
    }

    fn buy(&mut self, skip_scan: bool) -> Value {
        if !skip_scan {
            match &self.scan {
                None => {
                    return self.note(
                        BUYER,
                        false,
                        "buy -> refused by the buyer's tools: scan first".into(),
                    )
                }
                Some(r) if r.blocks() => {
                    return self.note(
                        BUYER,
                        false,
                        format!("buy -> SCAN_BLOCKED: the scan said {}", r.verdict),
                    )
                }
                _ => {}
            }
            // Like the SDK and MCP buy_agent: scan again right before paying,
            // so a key or code change after an earlier clean scan still blocks.
            let fresh = scan::scan(&self.scan_input());
            if fresh.blocks() {
                let codes = fresh.codes().join(", ");
                let verdict = fresh.verdict.clone();
                self.scan = Some(fresh);
                return self.note(
                    BUYER,
                    false,
                    format!("buy -> SCAN_BLOCKED: the re-scan before paying said {verdict} ({codes}); the account changed after the earlier scan"),
                );
            }
        }
        let Some(l) = self.m.listing.clone() else {
            return self.note(
                BUYER,
                false,
                format!("buy -> {}", agent_core::Error::NotListed),
            );
        };
        let hash = self.m.known_keys_hash();
        let plan = match self.m.begin_buy_sealed(
            BUYER,
            l.price,
            l.version,
            &hash,
            BUYER_KEY,
            Some(&agent_core::sealed::to_hex(
                &agent_core::sealed::public_key(&BOB_DELIVERY_SK),
            )),
            true,
            self.now,
        ) {
            Ok(p) => p,
            Err(e) => return self.note(BUYER, false, format!("buy -> {e}")),
        };
        // The key batch on the agent account: delete the seller's runner keys, add the buyer's.
        self.keys
            .retain(|k| !plan.delete_keys.contains(&k.public_key));
        self.keys.push(runner_key(BUYER_KEY));
        match self.m.finish_buy(plan.sale_id, true, self.now) {
            Ok(BuyResult::Done {
                sale,
                payout,
                fee,
                seller,
                ..
            }) => {
                self.m
                    .transfer_done(TransferKind::SellerPayout, &seller, None, payout, true);
                self.scan = None;
                let left = if skip_scan && self.keys.iter().any(|k| k.public_key == HIDDEN_KEY) {
                    " The hidden key is still on the account: the scan was skipped."
                } else {
                    ""
                };
                if sale.paid_now == 0 {
                    return self.note(BUYER, true, format!(
                        "buy {} -> one transaction: owner is now bob, alice's key deleted, bob's key added; 0.00 paid to alice now: all {} is held and reaches her only from customer payments after the sale (pay from revenue).{left}",
                        usd(sale.price), usd(sale.escrow)));
                }
                self.note(BUYER, true, format!(
                    "buy {} -> one transaction: owner is now bob, alice's key deleted, bob's key added; {} paid to alice now (after the 1% protocol fee, {}), {} held as earn-out.{left}",
                    usd(sale.price), usd(payout), usd(fee), usd(sale.escrow)))
            }
            Ok(BuyResult::RolledBack { .. }) => {
                self.note(BUYER, false, "buy -> rolled back, full refund".into())
            }
            Err(e) => self.note(BUYER, false, format!("buy -> {e}")),
        }
    }

    pub fn view(&self) -> Value {
        let e = self.m.earnings(self.now);
        let label = |pk: &str| match pk {
            SELLER_KEY => "alice's runner key",
            BUYER_KEY => "bob's runner key",
            HIDDEN_KEY => "hidden backdoor key",
            _ => "unknown key",
        };
        let sale = self.m.sales.last().map(|s| {
            json!({
                "price": usd(s.price), "paid_now": usd(s.paid_now), "escrow": usd(s.escrow),
                "released": usd(s.released), "returned": usd(s.returned), "finalized": s.finalized,
            })
        });
        json!({
            "agent": AGENT,
            "owner": self.m.owner,
            "status": format!("{:?}", self.m.status),
            "frozen": self.m.frozen,
            "listing": self.m.listing.as_ref().map(|l| json!({"price": usd(l.price), "version": l.version})),
            "earnings": {"gross": usd(e.gross), "counted": usd(e.counted), "verified": usd(e.verified), "flagged": usd(e.flagged)},
            "keys": self.keys.iter().map(|k| json!({
                "key": k.public_key, "label": label(&k.public_key), "full_access": k.is_full(),
                "known_to_contract": self.m.known_keys.contains(&k.public_key),
            })).collect::<Vec<_>>(),
            "upgrade_method": self.upgrade_method,
            "taken_over": self.taken_over,
            "scan": self.scan,
            "sale": sale,
            "sealed_config": self.m.sales.last().and_then(|s| s.config.as_ref()).map(|c| json!({"commit": c.commit, "state": c.state}))
                .or_else(|| self.m.config_commit.as_ref().map(|c| json!({"commit": c, "state": "committed"}))),
            "escrow_left": usd(self.m.escrow_left()),
            "claimable": {"alice": usd(self.m.claimable_of(SELLER)), "bob": usd(self.m.claimable_of(BUYER)),
                          "dave": usd(self.m.claimable_of(INVESTOR))},
            "shares": self.m.offers.iter().map(|o| json!({
                "id": o.id, "pct": o.bps as f64 / 100.0, "price": usd(o.price), "sold_pct": o.sold_bps as f64 / 100.0,
                "seller": o.seller, "escrow": usd(o.escrow()),
                "holders": o.holders.iter().map(|h| json!({"account": h.account, "pct": h.bps as f64 / 100.0,
                    "paid_in": usd(h.paid_in), "earned": usd(h.earned), "escrow": usd(h.escrow)})).collect::<Vec<_>>(),
            })).collect::<Vec<_>>(),
            "owner_free": usd(self.m.owner_free),
            "solvent": self.m.solvent(),
            "log": self.log,
        })
    }
}

/// One action in, the result plus the whole view out.
pub fn handle(w: &mut World, input: &str) -> String {
    let out = match serde_json::from_str::<Action>(input) {
        Ok(a) => {
            let last = w.act(a);
            json!({"ok": true, "last": last, "view": w.view()})
        }
        Err(e) => json!({"ok": false, "error": format!("bad action: {e}")}),
    };
    out.to_string()
}

thread_local! {
    static WORLD_STATE: RefCell<Option<World>> = const { RefCell::new(None) };
    static IN: RefCell<Vec<u8>> = const { RefCell::new(Vec::new()) };
    static OUT: RefCell<Vec<u8>> = const { RefCell::new(Vec::new()) };
}

/// A buffer of `len` bytes for the next action; returns its address.
#[no_mangle]
pub extern "C" fn pg_alloc(len: usize) -> *mut u8 {
    IN.with(|b| {
        let mut b = b.borrow_mut();
        b.clear();
        b.resize(len, 0);
        b.as_mut_ptr()
    })
}

/// Runs the action in the input buffer; returns the answer's length.
#[no_mangle]
pub extern "C" fn pg_call() -> usize {
    let input = IN.with(|b| String::from_utf8_lossy(&b.borrow()).into_owned());
    let out = WORLD_STATE.with(|w| {
        let mut w = w.borrow_mut();
        handle(w.get_or_insert_with(World::new), &input)
    });
    OUT.with(|o| {
        let mut o = o.borrow_mut();
        *o = out.into_bytes();
        o.len()
    })
}

/// Address of the last answer.
#[no_mangle]
pub extern "C" fn pg_out() -> *const u8 {
    OUT.with(|o| o.borrow().as_ptr())
}

/// No randomness in the browser build: every rule is deterministic (see Cargo.toml).
#[cfg(target_arch = "wasm32")]
fn no_random(_: &mut [u8]) -> Result<(), getrandom::Error> {
    Err(getrandom::Error::UNSUPPORTED)
}
#[cfg(target_arch = "wasm32")]
getrandom::register_custom_getrandom!(no_random);
