//! Seeded attack corpus for `make attack`: clean agents plus mutated agents,
//! one mutation family per attack id (SCENARIOS.md A-series). Same seed, same
//! corpus, same report.

use crate::*;
use serde_json::{json, Value};

/// The published code hash used by the corpus and fixtures. The real value is
/// the reproducible-build hash in contracts/res/SHA256SUMS (batch B3).
pub const FIXTURE_CODE_HASH: &str = "fixture-release-hash-v1";
pub const NOW_NS: u64 = 1_767_225_600_000_000_000 + 10 * 86_400_000_000_000;
pub const TOKEN: &str = "xusd.testnet";

/// splitmix64: tiny, dependency-free, deterministic.
pub struct Rng(u64);

impl Rng {
    pub fn new(seed: u64) -> Self {
        Rng(seed)
    }
    pub fn next_u64(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }
    pub fn below(&mut self, n: u64) -> u64 {
        self.next_u64() % n
    }
    pub fn pick<'a, T>(&mut self, v: &'a [T]) -> &'a T {
        &v[self.below(v.len() as u64) as usize]
    }
    pub fn key(&mut self) -> String {
        let mut b = [0u8; 32];
        for c in b.chunks_mut(8) {
            c.copy_from_slice(&self.next_u64().to_le_bytes()[..c.len()]);
        }
        format!("ed25519:{}", bs58::encode(b).into_string())
    }
}

pub fn runner_key(agent: &str, pk: &str) -> AccessKey {
    AccessKey {
        public_key: pk.into(),
        permission: Permission::FunctionCall {
            receiver_id: agent.into(),
            method_names: RUNNER_METHODS.iter().map(|s| s.to_string()).collect(),
            allowance: Some("5000000000000000000000000".into()),
        },
    }
}

/// A clean listed agent: one runner key added by the contract, no full-access
/// key, published code, no approvals.
pub fn clean_agent(rng: &mut Rng, idx: usize) -> ScanInput {
    let agent = format!("agent{idx}.testnet");
    let pk = rng.key();
    let version = 1 + rng.below(5);
    let known = vec![pk.clone()];
    let subaccounts = if rng.below(4) == 0 {
        vec![format!("data.{agent}")]
    } else {
        vec![]
    };
    ScanInput {
        schema: INPUT_SCHEMA.into(),
        agent: agent.clone(),
        now_ns: NOW_NS,
        expected: Expected {
            code_hash: FIXTURE_CODE_HASH.into(),
            listing_version: version,
            admin: None,
            token: None,
            price: None,
            min_terms: None,
        },
        chain: Chain {
            code_hash: FIXTURE_CODE_HASH.into(),
            methods: PUBLISHED_METHODS.iter().map(|s| s.to_string()).collect(),
            keys: vec![runner_key(&agent, &pk)],
            keys_second_provider: Some(vec![runner_key(&agent, &pk)]),
            subaccounts,
        },
        contract: ContractView {
            status: "Listed".into(),
            owner: format!("seller{idx}.testnet"),
            listing_version: version,
            known_keys_hash: agent_core::keys_hash(known.iter()),
            known_keys: known,
            open_approvals: 0,
            open_auth_until: if rng.below(3) == 0 { NOW_NS - 1 } else { 0 },
            frozen: false,
            admin: Some(PUBLISHED_ADMIN.into()),
            token: Some(PUBLISHED_TOKEN.into()),
            paid_to_total: Some(rng.below(1_500)),
        },
        listing: Some(ListingView {
            listing_version: version,
            price: ((100 + rng.below(1_900)) * 1_000_000).to_string(),
            terms: TermsView {
                earnout_bps: 3_000 + 500 * rng.below(5) as u16,
                share_bps: 5_000,
                floor_bps: 2_000,
                window_secs: 30 * 86_400 * (1 + rng.below(3)),
            },
        }),
    }
}

pub struct Family {
    pub id: &'static str,
    pub name: &'static str,
    /// The finding code that must appear.
    pub expect: &'static str,
    pub mutate: fn(&mut ScanInput, &mut Rng),
}

fn add_key_both(i: &mut ScanInput, k: AccessKey) {
    i.chain.keys.push(k.clone());
    if let Some(s) = &mut i.chain.keys_second_provider {
        s.push(k);
    }
}

const OWNER_LIKE: [&str; 5] = [
    "withdraw",
    "nft_transfer",
    "set_runner_key",
    "claim",
    "list_for_sale",
];

pub const FAMILIES: [Family; 19] = [
    Family {
        id: "A1",
        name: "extra full-access key",
        expect: "full_access_key_present",
        mutate: |i, r| {
            let k = r.key();
            add_key_both(
                i,
                AccessKey {
                    public_key: k,
                    permission: Permission::Full("FullAccess".into()),
                },
            );
        },
    },
    Family {
        id: "A2",
        name: "extra function-call key to the agent with owner-like methods",
        expect: "unknown_key",
        mutate: |i, r| {
            let n = 1 + r.below(3) as usize;
            let methods = (0..n).map(|_| r.pick(&OWNER_LIKE).to_string()).collect();
            let k = AccessKey {
                public_key: r.key(),
                permission: Permission::FunctionCall {
                    receiver_id: i.agent.clone(),
                    method_names: methods,
                    allowance: None,
                },
            };
            add_key_both(i, k);
        },
    },
    Family {
        id: "A3",
        name: "extra function-call key to a foreign contract",
        expect: "unknown_key",
        mutate: |i, r| {
            let recv = r
                .pick(&[TOKEN, "wrap.testnet", "v1.signer-prod.testnet"])
                .to_string();
            let k = AccessKey {
                public_key: r.key(),
                permission: Permission::FunctionCall {
                    receiver_id: recv,
                    method_names: vec!["ft_transfer".into()],
                    allowance: None,
                },
            };
            add_key_both(i, k);
        },
    },
    Family {
        id: "A4",
        name: "runner-looking key added with the setup key, not by the contract",
        expect: "unknown_key",
        mutate: |i, r| {
            let k = runner_key(&i.agent.clone(), &r.key());
            add_key_both(i, k);
        },
    },
    Family {
        id: "A5",
        name: "patched wasm (code hash differs)",
        expect: "code_hash_mismatch",
        mutate: |i, r| {
            i.chain.code_hash = format!("patched-{:016x}", r.next_u64());
        },
    },
    Family {
        id: "A6",
        name: "same interface plus an upgrade / self-deploy method",
        expect: "upgrade_method",
        mutate: |i, r| {
            let m = r
                .pick(&[
                    "upgrade",
                    "deploy_code",
                    "migrate",
                    "add_full_access_key",
                    "owner_add_key",
                    "update_contract",
                ])
                .to_string();
            i.chain.methods.push(m);
            if r.below(2) == 0 {
                i.chain.code_hash = format!("patched-{:016x}", r.next_u64());
            }
        },
    },
    Family {
        id: "A6x",
        name: "extra method that is not in the published contract",
        expect: "unexpected_method",
        mutate: |i, r| {
            let m = r
                .pick(&["drain", "set_owner", "emergency_withdraw", "debug_call"])
                .to_string();
            i.chain.methods.push(m);
        },
    },
    Family {
        id: "A7",
        name: "seller changed terms after the buyer was quoted",
        expect: "listing_version_mismatch",
        mutate: |i, r| {
            i.contract.listing_version += 1 + r.below(3);
        },
    },
    Family {
        id: "A8",
        name: "open approval on the deed",
        expect: "open_approval",
        mutate: |i, r| {
            i.contract.open_approvals = 1 + r.below(3);
        },
    },
    Family {
        id: "A9",
        name: "pre-signed Base authorization still valid",
        expect: "open_base_auth",
        mutate: |i, r| {
            i.contract.open_auth_until = i.now_ns + 1 + r.below(3_600_000_000_000);
        },
    },
    Family {
        id: "A21",
        name: "setup full-access key never deleted",
        expect: "full_access_key_present",
        mutate: |i, r| {
            let k = r.key();
            i.chain.keys.insert(
                0,
                AccessKey {
                    public_key: k.clone(),
                    permission: Permission::Full("FullAccess".into()),
                },
            );
            if let Some(s) = &mut i.chain.keys_second_provider {
                s.insert(
                    0,
                    AccessKey {
                        public_key: k,
                        permission: Permission::Full("FullAccess".into()),
                    },
                );
            }
        },
    },
    Family {
        id: "E31",
        name: "one RPC provider hides a key",
        expect: "rpc_disagree",
        mutate: |i, r| {
            let k = AccessKey {
                public_key: r.key(),
                permission: Permission::Full("FullAccess".into()),
            };
            // Provider 1 (the one a lazy client reads) hides the key.
            if let Some(s) = &mut i.chain.keys_second_provider {
                s.push(k);
            }
        },
    },
    Family {
        id: "K1",
        name: "runner key scope widened beyond runner methods",
        expect: "runner_key_scope",
        mutate: |i, r| {
            let extra = r.pick(&OWNER_LIKE).to_string();
            for list in
                std::iter::once(&mut i.chain.keys).chain(i.chain.keys_second_provider.iter_mut())
            {
                if let Some(AccessKey {
                    permission: Permission::FunctionCall { method_names, .. },
                    ..
                }) = list.first_mut()
                {
                    method_names.push(extra.clone());
                }
            }
        },
    },
    Family {
        id: "K2",
        name: "contract lists a key the account does not have",
        expect: "known_key_missing",
        mutate: |i, r| {
            let k = r.key();
            i.contract.known_keys.push(k);
            i.contract.known_keys_hash = agent_core::keys_hash(i.contract.known_keys.iter());
        },
    },
    Family {
        id: "K3",
        name: "known_keys_hash forged",
        expect: "known_keys_hash_mismatch",
        mutate: |i, r| {
            i.contract.known_keys_hash = format!("{:064x}", r.next_u64());
        },
    },
    Family {
        id: "A23",
        name: "agent already frozen (backdoor proven)",
        expect: "frozen",
        mutate: |i, _| {
            i.contract.frozen = true;
        },
    },
    Family {
        id: "A26",
        name: "deployed with the seller's own admin or payment token",
        expect: "registry_mismatch",
        mutate: |i, r| match r.below(3) {
            0 => i.contract.admin = Some(i.contract.owner.clone()),
            1 => i.contract.token = Some(format!("fake{}.testnet", r.below(100))),
            _ => i.contract.admin = None,
        },
    },
    Family {
        id: "A32",
        name: "earn-out terms below the buyer's minimum (no escrow, seller floor, short window)",
        expect: "terms_below_minimum",
        mutate: |i, r| {
            let t = &mut i
                .listing
                .as_mut()
                .expect("clean agents have a listing")
                .terms;
            match r.below(3) {
                0 => t.earnout_bps = r.below(3_000) as u16,
                1 => t.floor_bps = 5_001 + r.below(5_000) as u16,
                _ => t.window_secs = 60 + r.below(30 * 86_400 - 60),
            }
        },
    },
    Family {
        id: "A36",
        name: "seller filled the round-trip table so the buyer inherits PaidToFull",
        expect: "paid_to_bloated",
        mutate: |i, r| {
            i.contract.paid_to_total = match r.below(4) {
                0 => None,
                _ => Some(PAID_TO_TOTAL - PAID_TO_QUOTA + 1 + r.below(PAID_TO_QUOTA)),
            }
        },
    },
];

#[derive(Debug, Clone)]
pub struct Case {
    pub family: &'static str,
    pub expect: &'static str,
    pub input: ScanInput,
}

/// `n` mutated agents spread round-robin over the families (each with its
/// own random base), plus `clean_n` clean controls.
pub fn generate(seed: u64, n: usize, clean_n: usize) -> (Vec<Case>, Vec<ScanInput>) {
    let mut rng = Rng::new(seed);
    let mut cases = Vec::with_capacity(n);
    for idx in 0..n {
        let fam = &FAMILIES[idx % FAMILIES.len()];
        let mut input = clean_agent(&mut rng, idx);
        (fam.mutate)(&mut input, &mut rng);
        cases.push(Case {
            family: fam.id,
            expect: fam.expect,
            input,
        });
    }
    let clean = (0..clean_n)
        .map(|i| clean_agent(&mut rng, 10_000 + i))
        .collect();
    (cases, clean)
}

/// Run the corpus and build the `attack-report.v1` body (without the
/// contract-layer and judge sections, which the `attack` tool adds).
pub fn run(seed: u64, n: usize, clean_n: usize) -> Value {
    let (cases, clean) = generate(seed, n, clean_n);
    let mut fams = Vec::new();
    let mut total_caught = 0;
    let mut missed_examples = Vec::new();
    for fam in FAMILIES.iter() {
        let mine: Vec<&Case> = cases.iter().filter(|c| c.family == fam.id).collect();
        let mut caught = 0;
        for c in &mine {
            let r = scan(&c.input);
            if r.blocks() && r.codes().contains(&c.expect) {
                caught += 1;
            } else if missed_examples.len() < 5 {
                missed_examples
                    .push(json!({"family": c.family, "agent": c.input.agent, "report": r}));
            }
        }
        total_caught += caught;
        fams.push(
            json!({"id": fam.id, "attack": fam.name, "expected_finding": fam.expect,
                          "cases": mine.len(), "caught": caught, "missed": mine.len() - caught}),
        );
    }
    let mut fp = 0;
    let mut warned = 0;
    for c in &clean {
        let r = scan(c);
        if r.blocks() {
            fp += 1;
        }
        if !r.findings.is_empty() {
            warned += 1;
        }
    }
    let rate = if clean.is_empty() {
        0.0
    } else {
        fp as f64 / clean.len() as f64
    };
    json!({
        "seed": seed,
        "in_scope": {"cases": cases.len(), "caught": total_caught, "missed": cases.len() - total_caught},
        "families": fams,
        "clean": {"cases": clean.len(), "false_positives": fp, "false_positive_rate": rate,
                  "with_warnings_only": warned},
        "missed_examples": missed_examples,
        "not_caught_by_design": NOT_CAUGHT.iter()
            .map(|(id, what, why)| json!({"id": id, "not_caught": what, "why": why}))
            .collect::<Vec<_>>(),
    })
}
