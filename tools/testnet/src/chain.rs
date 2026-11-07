//! A thin client over near-api for testnet: every call returns a `Tx`
//! record (hash, explorer link, ok or the node's real error, logs, events)
//! that goes into out/. Secrets never enter a `Tx`.

use near_api::signer::generate_secret_key;
use near_api::types::{AccessKeyPermission, AccountId, NearGas, NearToken, SecretKey};
use near_api::{Account, Contract, NetworkConfig, Signer};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::sync::Arc;
use std::time::Instant;
use testnet_plan::{check_secret_mode, Config, EXPLORER_TX};

pub type Res<T> = Result<T, String>;

pub struct Chain {
    pub cfg: Config,
    pub net: NetworkConfig,
    pub net2: NetworkConfig,
    pub keys: KeyStore,
    pub root: Acct,
}

#[derive(Clone)]
pub struct Acct {
    pub id: AccountId,
    pub signer: Arc<Signer>,
    pub pk: String,
}

/// One transaction as recorded in out/.
#[derive(Debug, Clone)]
pub struct Tx {
    pub label: String,
    pub by: String,
    pub receiver: String,
    pub method: String,
    pub hash: Option<String>,
    pub ok: bool,
    pub error: Option<String>,
    pub value: Value,
    pub logs: Vec<String>,
    pub ms: u128,
}

impl Tx {
    pub fn events(&self, name: &str) -> Vec<Value> {
        self.logs
            .iter()
            .filter_map(|l| l.strip_prefix("EVENT_JSON:"))
            .filter_map(|j| serde_json::from_str::<Value>(j).ok())
            .filter(|v| v["event"] == name)
            .map(|v| v["data"][0].clone())
            .collect()
    }

    pub fn json(&self) -> Value {
        let events: Vec<Value> = self
            .logs
            .iter()
            .filter_map(|l| l.strip_prefix("EVENT_JSON:"))
            .filter_map(|j| serde_json::from_str::<Value>(j).ok())
            .collect();
        json!({
            "label": self.label, "by": self.by, "receiver": self.receiver, "method": self.method,
            "ok": self.ok, "error": self.error, "value": self.value, "events": events,
            "tx": self.hash, "explorer": self.hash.as_ref().map(|h| format!("{EXPLORER_TX}{h}")),
            "ms": self.ms,
        })
    }

    /// Error text, or "" if it succeeded.
    pub fn err(&self) -> &str {
        self.error.as_deref().unwrap_or("")
    }
}

/// The node's "this key is not on the account" errors (A17, A21).
pub fn is_missing_key(err: &str) -> bool {
    [
        "AccessKeyNotFound",
        "does not exist",
        "UnknownAccessKey",
        "InvalidAccessKeyError",
    ]
    .iter()
    .any(|n| err.contains(n))
}

fn short(s: String) -> String {
    if s.len() > 1_500 {
        format!("{}...", &s[..1_500])
    } else {
        s
    }
}

/// Keys this runner creates, one file per name in `keys_dir` (0600).
pub struct KeyStore {
    pub dir: String,
}

impl KeyStore {
    fn path(&self, name: &str) -> String {
        format!("{}/{name}.key", self.dir)
    }

    pub fn get(&self, name: &str) -> Option<SecretKey> {
        let t = std::fs::read_to_string(self.path(name)).ok()?;
        t.trim().parse().ok()
    }

    /// The key named `name`, created (and saved, 0600) if missing.
    pub fn get_or_create(&self, name: &str) -> Res<SecretKey> {
        self.check()?;
        if let Some(k) = self.get(name) {
            return Ok(k);
        }
        use std::os::unix::fs::{DirBuilderExt, OpenOptionsExt};
        std::fs::DirBuilder::new()
            .recursive(true)
            .mode(0o700)
            .create(&self.dir)
            .map_err(|e| format!("{}: {e}", self.dir))?;
        let sk = generate_secret_key().map_err(|e| format!("{e:?}"))?;
        use std::io::Write;
        let mut f = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(self.path(name))
            .map_err(|e| format!("{}: {e}", self.path(name)))?;
        writeln!(f, "{sk}").map_err(|e| e.to_string())?;
        Ok(sk)
    }

    /// Refuse a key directory or key file that group or others can read.
    pub fn check(&self) -> Res<()> {
        check_secret_mode(&self.dir)?;
        for e in std::fs::read_dir(&self.dir).into_iter().flatten().flatten() {
            if let Some(p) = e.path().to_str() {
                check_secret_mode(p)?;
            }
        }
        Ok(())
    }

    /// Every secret value held here (to make sure none reaches out/).
    pub fn all_secrets(&self) -> Vec<String> {
        std::fs::read_dir(&self.dir)
            .into_iter()
            .flatten()
            .flatten()
            .filter_map(|e| std::fs::read_to_string(e.path()).ok())
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
            .collect()
    }
}

pub fn acct(id: &str, sk: &SecretKey) -> Res<Acct> {
    Ok(Acct {
        id: id.parse().map_err(|e| format!("{id}: {e:?}"))?,
        signer: Signer::from_secret_key(sk.clone()).map_err(|e| format!("{e:?}"))?,
        pk: sk.public_key().to_string(),
    })
}

pub fn sha256_b58(b: &[u8]) -> String {
    bs58::encode(Sha256::digest(b)).into_string()
}

impl Chain {
    pub fn id(&self, name: &str) -> String {
        self.cfg.account(name)
    }

    /// The account `name.ROOT` signed by its own stored key.
    pub fn acct(&self, name: &str) -> Res<Acct> {
        let sk = self
            .keys
            .get(name)
            .ok_or(format!("no key for {name}: run make deploy-testnet first"))?;
        acct(&self.id(name), &sk)
    }

    /// The agent account `agent` signed by a runner key stored as `key_name`.
    pub fn runner(&self, agent: &str, key_name: &str) -> Res<Acct> {
        let sk = self
            .keys
            .get(key_name)
            .ok_or(format!("no key {key_name}"))?;
        acct(&self.id(agent), &sk)
    }

    #[allow(clippy::too_many_arguments)]
    pub async fn call(
        &self,
        label: &str,
        who: &Acct,
        receiver: &str,
        method: &str,
        args: Value,
        yocto: u128,
        tgas: u64,
    ) -> Tx {
        let t0 = Instant::now();
        let mut tx = Tx {
            label: label.into(),
            by: who.id.to_string(),
            receiver: receiver.into(),
            method: method.into(),
            hash: None,
            ok: false,
            error: None,
            value: Value::Null,
            logs: vec![],
            ms: 0,
        };
        let receiver_id: AccountId = match receiver.parse() {
            Ok(r) => r,
            Err(e) => {
                tx.error = Some(format!("{e:?}"));
                return tx;
            }
        };
        // A node refuses a function-call key whose allowance cannot cover
        // the attached gas at its pessimistic price, and the error comes back
        // as a bare `InvalidTransaction({})`. Check first and say why.
        if let Some(e) = self.allowance_short(who, tgas).await {
            println!("  FAIL {label:<40} {e}");
            tx.error = Some(e);
            return tx;
        }
        let sent = Contract(receiver_id)
            .call_function(method, args)
            .transaction()
            .gas(NearGas::from_tgas(tgas))
            .deposit(NearToken::from_yoctonear(yocto))
            .with_signer(who.id.clone(), who.signer.clone())
            .send_to(&self.net)
            .await;
        tx.ms = t0.elapsed().as_millis();
        match sent {
            // Refused before execution (e.g. the key is not on the account).
            Err(e) => tx.error = Some(short(format!("{e:?}"))),
            Ok(r) => match r.into_full() {
                None => tx.error = Some("pending: no final outcome".into()),
                Some(full) => {
                    // The id NEAR returns for the transaction outcome: recomputing the hash
                    // from the decoded transaction gives a different value.
                    tx.hash = Some(full.outcome().transaction_hash.to_string());
                    tx.logs = full.logs().iter().map(|l| l.to_string()).collect();
                    match full.into_result() {
                        Ok(ok) => {
                            tx.ok = true;
                            tx.value = ok.json::<Value>().unwrap_or(Value::Null);
                        }
                        Err(f) => tx.error = Some(short(format!("{f}"))),
                    }
                }
            },
        }
        println!(
            "  {} {:<40} {}",
            if tx.ok { "ok  " } else { "FAIL" },
            label,
            tx.error
                .as_deref()
                .map(|e| e.chars().take(140).collect::<String>())
                .unwrap_or_default()
        );
        tx
    }

    /// A call whose input is raw bytes (the factory's `store_code` takes the
    /// wasm itself). Signed by a full-access key, so no allowance check.
    pub async fn call_raw(
        &self,
        label: &str,
        who: &Acct,
        receiver: &str,
        method: &str,
        bytes: Vec<u8>,
        tgas: u64,
    ) -> Tx {
        let t0 = Instant::now();
        let mut tx = Tx {
            label: label.into(),
            by: who.id.to_string(),
            receiver: receiver.into(),
            method: method.into(),
            hash: None,
            ok: false,
            error: None,
            value: Value::Null,
            logs: vec![],
            ms: 0,
        };
        let receiver_id: AccountId = match receiver.parse() {
            Ok(r) => r,
            Err(e) => {
                tx.error = Some(format!("{e:?}"));
                return tx;
            }
        };
        let sent = Contract(receiver_id)
            .call_function_raw(method, bytes)
            .transaction()
            .gas(NearGas::from_tgas(tgas))
            .with_signer(who.id.clone(), who.signer.clone())
            .send_to(&self.net)
            .await;
        tx.ms = t0.elapsed().as_millis();
        match sent {
            Err(e) => tx.error = Some(short(format!("{e:?}"))),
            Ok(r) => match r.into_full() {
                None => tx.error = Some("pending: no final outcome".into()),
                Some(full) => {
                    tx.hash = Some(full.outcome().transaction_hash.to_string());
                    tx.logs = full.logs().iter().map(|l| l.to_string()).collect();
                    match full.into_result() {
                        Ok(_) => tx.ok = true,
                        Err(f) => tx.error = Some(short(format!("{f}"))),
                    }
                }
            },
        }
        println!(
            "  {} {:<40} {}",
            if tx.ok { "ok  " } else { "FAIL" },
            label,
            tx.error.as_deref().unwrap_or_default()
        );
        tx
    }

    /// `Some(reason)` if `who` signs with a function-call key whose
    /// allowance is below what the node reserves for `tgas` (attached gas at
    /// a pessimistic 1e9 yocto per gas, 10x the testnet minimum price).
    pub async fn allowance_short(&self, who: &Acct, tgas: u64) -> Option<String> {
        let keys = self.keys(who.id.as_str()).await.ok()?;
        let k = keys.iter().find(|k| k.public_key == who.pk)?;
        let scan::Permission::FunctionCall {
            allowance: Some(a), ..
        } = &k.permission
        else {
            return None;
        };
        let have: u128 = a.parse().ok()?;
        let need = tgas as u128 * 1_000_000_000_000 * 1_000_000_000;
        (have < need).then(|| {
            format!(
                "AllowanceTooLow: key {} has {} yoctoNEAR of gas allowance, {tgas} TGas needs about {need}; the owner refreshes it with set_runner_key",
                who.pk, have
            )
        })
    }

    pub async fn view_on(
        &self,
        net: &NetworkConfig,
        contract: &str,
        method: &str,
        args: Value,
    ) -> Res<Value> {
        let id: AccountId = contract.parse().map_err(|e| format!("{e:?}"))?;
        Contract(id)
            .call_function(method, args)
            .read_only::<Value>()
            .fetch_from(net)
            .await
            .map(|d| d.data)
            .map_err(|e| short(format!("{contract}.{method}: {e:?}")))
    }

    pub async fn view(&self, contract: &str, method: &str, args: Value) -> Res<Value> {
        self.view_on(&self.net, contract, method, args).await
    }

    pub async fn exists(&self, id: &str) -> bool {
        match id.parse::<AccountId>() {
            Ok(a) => Account(a).view().fetch_from(&self.net).await.is_ok(),
            Err(_) => false,
        }
    }

    pub async fn balance_near(&self, id: &str) -> Res<u128> {
        let a: AccountId = id.parse().map_err(|e| format!("{e:?}"))?;
        Account(a)
            .view()
            .fetch_from(&self.net)
            .await
            .map(|d| d.data.amount.as_yoctonear())
            .map_err(|e| short(format!("{e:?}")))
    }

    /// Key list as the scan reads it, from one RPC.
    pub async fn keys_on(&self, net: &NetworkConfig, id: &str) -> Res<Vec<scan::AccessKey>> {
        let a: AccountId = id.parse().map_err(|e| format!("{e:?}"))?;
        let list = Account(a)
            .list_keys()
            .fetch_from(net)
            .await
            .map_err(|e| short(format!("{e:?}")))?
            .data;
        let mut v: Vec<scan::AccessKey> = list
            .into_iter()
            .map(|(pk, ak)| scan::AccessKey {
                public_key: pk.to_string(),
                permission: match ak.permission {
                    AccessKeyPermission::FullAccess => scan::Permission::Full("FullAccess".into()),
                    AccessKeyPermission::FunctionCall(f) => scan::Permission::FunctionCall {
                        receiver_id: f.receiver_id,
                        method_names: f.method_names,
                        allowance: f.allowance.map(|a| a.as_yoctonear().to_string()),
                    },
                },
            })
            .collect();
        v.sort_by(|a, b| a.public_key.cmp(&b.public_key));
        Ok(v)
    }

    pub async fn keys(&self, id: &str) -> Res<Vec<scan::AccessKey>> {
        self.keys_on(&self.net, id).await
    }

    /// The deployed code: (base58 sha256, bytes).
    pub async fn code(&self, id: &str) -> Res<(String, Vec<u8>)> {
        use base64::Engine;
        let a: AccountId = id.parse().map_err(|e| format!("{e:?}"))?;
        let c = Contract(a)
            .wasm()
            .fetch_from(&self.net)
            .await
            .map_err(|e| short(format!("{e:?}")))?
            .data;
        let bytes = base64::engine::general_purpose::STANDARD
            .decode(c.code_base64)
            .map_err(|e| e.to_string())?;
        Ok((sha256_b58(&bytes), bytes))
    }

    /// Create `id` funded by `parent` with a new key stored as `key_name`.
    pub async fn create(
        &self,
        label: &str,
        parent: &Acct,
        id: &str,
        key_name: &str,
        milli_near: u32,
    ) -> Res<Tx> {
        let sk = self.keys.get_or_create(key_name)?;
        let t0 = Instant::now();
        let r = Account::create_account(id.parse().map_err(|e| format!("{e:?}"))?)
            .fund_myself(
                parent.id.clone(),
                NearToken::from_millinear(milli_near as u128),
            )
            .with_public_key(sk.public_key())
            .with_signer(parent.signer.clone())
            .send_to(&self.net)
            .await;
        Ok(self.record(
            label,
            parent.id.as_ref(),
            id,
            "create_account",
            r.map_err(|e| format!("{e:?}")),
            t0,
        ))
    }

    pub async fn deploy(
        &self,
        label: &str,
        who: &Acct,
        wasm: Vec<u8>,
        init_args: Value,
    ) -> Res<Tx> {
        let t0 = Instant::now();
        let r = Contract::deploy(who.id.clone())
            .use_code(wasm)
            .with_init_call("new", init_args)
            .map_err(|e| format!("{e:?}"))?
            .with_signer(who.signer.clone())
            .send_to(&self.net)
            .await;
        Ok(self.record(
            label,
            who.id.as_ref(),
            who.id.as_ref(),
            "deploy+new",
            r.map_err(|e| format!("{e:?}")),
            t0,
        ))
    }

    pub async fn delete_key(&self, label: &str, who: &Acct, pk: &str) -> Res<Tx> {
        let t0 = Instant::now();
        let key: near_api::types::PublicKey = pk.parse().map_err(|e| format!("{e:?}"))?;
        let r = Account(who.id.clone())
            .delete_key(key)
            .with_signer(who.signer.clone())
            .send_to(&self.net)
            .await;
        Ok(self.record(
            label,
            who.id.as_ref(),
            who.id.as_ref(),
            "delete_key",
            r.map_err(|e| format!("{e:?}")),
            t0,
        ))
    }

    fn record(
        &self,
        label: &str,
        by: &str,
        receiver: &str,
        method: &str,
        r: Result<near_api::types::transaction::result::TransactionResult, String>,
        t0: Instant,
    ) -> Tx {
        let mut tx = Tx {
            label: label.into(),
            by: by.into(),
            receiver: receiver.into(),
            method: method.into(),
            hash: None,
            ok: false,
            error: None,
            value: Value::Null,
            logs: vec![],
            ms: t0.elapsed().as_millis(),
        };
        match r {
            Err(e) => tx.error = Some(short(e)),
            Ok(r) => match r.into_full() {
                None => tx.error = Some("pending".into()),
                Some(full) => {
                    // The id NEAR returns for the transaction outcome: recomputing the hash
                    // from the decoded transaction gives a different value.
                    tx.hash = Some(full.outcome().transaction_hash.to_string());
                    tx.logs = full.logs().iter().map(|l| l.to_string()).collect();
                    match full.into_result() {
                        Ok(_) => tx.ok = true,
                        Err(f) => tx.error = Some(short(format!("{f}"))),
                    }
                }
            },
        }
        println!(
            "  {} {:<40} {}",
            if tx.ok { "ok  " } else { "FAIL" },
            label,
            tx.error
                .as_deref()
                .map(|e| e.chars().take(140).collect::<String>())
                .unwrap_or_default()
        );
        tx
    }
}
