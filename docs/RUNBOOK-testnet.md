# Runbook: Agent Exit on NEAR testnet

How to set up a machine, deploy, seed, run the gates and replay the six demo
scenes on testnet. Written from the first full run on 2026-11-02 (Windows 11 +
WSL Ubuntu 24.04, main at 98bde26, root `agentexit.testnet`). Testnet only:
the tools refuse any other network. Nothing here touches mainnet.

## 1. Machine (once)

Inside WSL Ubuntu 24.04:

| Tool | Version used | Install |
|---|---|---|
| build tools | gcc 13.3, make 4.3, pkg-config 1.8.1 | `sudo apt-get install -y build-essential pkg-config libssl-dev libudev-dev git make jq curl` |
| Rust | rustup 1.29; the repo pins 1.97.0 | `curl https://sh.rustup.rs -sSf \| sh -s -- -y --profile minimal` then `rustup target add wasm32-unknown-unknown` |
| cargo-near | 0.22.0 | `cargo install cargo-near --version 0.22.0 --locked` (needs `libudev-dev`, see pitfalls) |
| Node | 24 via nvm | `nvm install 24` |
| near CLI | near-cli-rs 0.30.1 | `npm i -g near-cli-rs@0.30.1` |
| Docker Desktop | 4.97, engine 29.9 | WSL integration on for Ubuntu-24.04; only needed for `make build` |
| Playwright | 1.64 | `npm i -g playwright && playwright install chromium` (only for `make web-test`) |

Clone into the Linux file system (`~/src/near-transparent-agent-market`), not
`/mnt/c`. Check: `make setup`, then `make verify`.

## 2. Accounts and secrets (once, by the owner)

1. Create the root account (free, sponsored by the faucet):
   `near account create-account sponsor-by-faucet-service agentexit.testnet autogenerate-new-keypair save-to-legacy-keychain network-config testnet create`
2. Fund it. The deploy needs **about 42 NEAR** (the factory 11.5, the factory
   creates translator with 10.2, the other accounts, 1 for margin); every other
   agent a gate or scene sells is created through the factory too and costs
   **10.2 NEAR** (G1, scene 8, scene 9, scene 10, each Exchange agent in scene 13).
   Plan on about 55 NEAR for deploy + G1 + scenes 3-6 and about 120 NEAR for
   everything (see `docs/REDEPLOY-NOTES.md`). Use near-faucet.io or the NEAR
   docs faucet (about 2 NEAR per request), or new faucet-sponsored accounts
   deleted with `beneficiary agentexit.testnet` (about 10 NEAR each).
3. Secrets live only in `~/.agent-exit.env` (chmod 600), never in the repo.
   See `.env.example`. What the first run used:
   - `AGENT_EXIT_NETWORK=testnet`
   - `AGENT_EXIT_ROOT=agentexit.testnet`
   - `AGENT_EXIT_ROOT_KEY=` copied from `~/.near-credentials/testnet/agentexit.testnet.json` without printing it:
     `echo "AGENT_EXIT_ROOT_KEY=$(jq -r .private_key ~/.near-credentials/testnet/agentexit.testnet.json)" >> ~/.agent-exit.env`
   - `AGENT_EXIT_RPC=https://rpc.testnet.fastnear.com`, `AGENT_EXIT_RPC_2=https://test.rpc.fastnear.com`
   - `NEARAI_API_KEY=` (only for gate G6 and `make llm-buyer`)
   - `BASE_RELAYER_KEY=` or `BASE_PAYEE=` (only for gate G2)
4. `chmod 600 ~/.agent-exit.env ~/.near-credentials/testnet/*.json`.

## 3. Build the published wasm

    make build                 # reproducible, in Docker; writes contracts/res + SHA256SUMS
    (cd contracts/res && sha256sum -c SHA256SUMS)

Current build (2026-11-13, root `agentexit6.testnet`, born-clean rule): agent_exit.wasm
sha256 `259d414a75fb83010140fa27cdc29e20cce79b4674600860ff2e5813a25e827e`, code hash
`3XqAPt3krfpJG9UWFHnZVyDYSws4ZnN4H3B27hCXPuuw`, built from commit `1dc6ca1`; the testnet
world runs exactly this committed build (see README). The root is compiled in, so
another root gives other hashes. Earlier builds: `5VqS52NR…` on `agentexit5` (2026-11-10),
`5dc306a2…` on `agentexit3` (2026-11-03) and the first run (2026-11-02, `agentexit.testnet`),
`be4b898d…` / `DoqGZ6zx…`, before the fixes.
`make build-testnet` gives different hashes (local toolchain); deploy and
scenes use contracts/res whenever SHA256SUMS exists.

The stale-build note of 2026-11-02 is resolved: the fixed build is deployed on `agentexit3.testnet`.

## 4. Deploy and seed

    make deploy-testnet DRY_RUN=1   # the plan, offline; nothing is sent
    make deploy-testnet             # create accounts, deploy the factory, create translator through it, runner keys, lock the factory, seed

Every step skips what is already done, so after any error fix the cause and
run the same command again. Afterwards `translator` and `cheat` each show
gross 500.00 (`get_earnings`), status Active, owner alice, solvent.
Output: `out/testnet/deploy.json`.

## 5. Gates

    make gate-g1     # key handover on a real node: about 10.5 NEAR (the factory creates a fresh agent)
    make gate-g6     # NEAR AI signature on-chain: needs NEAR AI credit
    make gate-g2     # Chain Signatures on Base Sepolia: needs BASE_RELAYER_KEY or BASE_PAYEE
                     # then make testnet-setup && make gate-g2-relay

First run: G1 PASS; G6 failed with HTTP 402 (no NEAR AI credit, skipped);
G2 not run (no Base Sepolia key). Output: `out/gates/`.

## 6. The six scenes

The links below are the current recording, on `agentexit6.testnet`; every scene, 1-13, gates G1 and G6, with its key transaction: [`docs/TESTNET.md`](TESTNET.md#scenes).

    make scenes      # recording order 3 4 5 1 2 6 -> out/scenes/

| Scene | What it shows | Proof |
|---|---|---|
| 3 | Verified vs unverified revenue: `get_earnings` of translator (360.00 of 500.00 counted, 140.00 flagged) next to cheat (all 500.00 flagged as self-paid) | out/scenes/scene-3.json (views only, no tx) |
| 4 | Cheat refused: alice (cheat is a plain deploy) calls `list_for_sale` and the contract answers `NotBornClean` (K8; the failed tx is recorded); a buy sent anyway is refunded; the scan (keys from both RPCs + `get_status`) blocks it with `full_access_key_present` and `contract_not_born_clean` | [BQrAUGB6…](https://testnet.nearblocks.io/txns/BQrAUGB6yMia6Ep4ZdTSWegoVgZqfP7LJPJV8Dnh99Pu) (list refused), [2Rf95RJf…](https://testnet.nearblocks.io/txns/2Rf95RJft2qHJbtJfXKYGihbnLjtFbpLinuL8wkgQP6H) (buy refunded) |
| 5 | The honest sale: bob buys `translator` in one transaction | [Hh9kTyAP…](https://testnet.nearblocks.io/txns/Hh9kTyAPxL8yMFAawj6rDtf2eGFtGZiXCKuRFnbwBkUs) |
| 1 | Break-in: the old seller is refused on-chain (withdraw, own key, re-list, `set_limits` NotOwner, deed move NotOwner, `pay_base` NotRunner); her old keys are rejected by the node | [set_limits refused](https://testnet.nearblocks.io/txns/CV5Jo1D4GwosqDDh8gFvDvmrXQbzv3UGnQk25gjs5odh) |
| 2 | Spending limits: cap 50.00, a pay of 8.00 passes, 45.00 is refused (PolicyViolation) | [refused pay](https://testnet.nearblocks.io/txns/FxS4XaWc7FBE4nzR1EH8QossqNf4Q16HKho8FanxiRZM) |
| 6 | An agent buys an agent: summarizer buys translator from bob | [summarizer buys](https://testnet.nearblocks.io/txns/FoEELL432dz35DJoS7LbXPa8nBAE1cMUykL8GBAP87RF) |

All outputs of the current run are committed in `out/testnet/`, `out/gates/` and `out/scenes/`.

## 7. Replay the demo

The scenes change the world (sales move the deed: after the first run
translator belongs to summarizer; cheat is a plain deploy the contract refuses to list). There is no reset
way back on the same root: a sale cannot be undone. To replay from a clean world:

1. Create and fund a new root (section 2), about 40 test NEAR, and put it in
   `~/.agent-exit.env` as `AGENT_EXIT_ROOT` and `AGENT_EXIT_ROOT_KEY`
   (for example `agentexit2.testnet`).
2. `make demo-setup`. It refuses a root that already played the scenes
   (FORCE=1 overrides), then runs `make build` (the root is compiled into the
   wasm, so the hash changes), `deploy-testnet`, `gate-g1`, `scenes` and
   `web-testnet-data`. `make demo-setup DRY_RUN=1` prints the plan offline.
3. Update the README testnet links and the hash in README, docs/VIDEO.md and
   section 3, then commit `out/`, `contracts/res` and `web/data`.

Re-running `make scenes` on the old world is not a replay; check
`get_status` of both agents first.

To show the recorded run without a chain: `make demo` and `make web`.

## 8. Pitfalls we hit

| Symptom | Cause | Fix |
|---|---|---|
| cargo-near install fails: `Unable to find libudev` | hidapi needs libudev | `apt-get install libudev-dev` |
| `docker` missing in WSL, folder mounts come back empty | Docker Desktop 4.30 WSL integration | upgrade Docker Desktop (4.97 works) |
| `make build`: missing `[package.metadata.near.reproducible_build]` | metadata not in Cargo.toml | fixed in 82d6e08 |
| deploy: `cannot update the lock file tools/testnet/Cargo.lock` | stale lock | fixed in d202319 |
| deploy: HTTP 429 on rpc.testnet.near.org | public RPC rate limit | `AGENT_EXIT_RPC=https://rpc.testnet.fastnear.com`, then re-run |
| seed: `The account doesn't have enough balance` (xUSD) | alice minted less than she pays | fixed in 5a477c8 |
| scene 5 blocked `code_hash_mismatch` / `unexpected_method` | scan compared with target/near-testnet and did not list the ABI/NEP-330 exports | fixed in 98bde26 |
| NEAR AI 401 | key pasted several times into a hidden prompt | paste once; check with a length-only test |
| NEAR AI 402 `no_limit_configured` | account has no credit | add credit at cloud.near.ai, or skip G6 |
| `make -n demo-setup` really deployed (2026-11-03, agentexit2) | GNU make runs recipe lines naming $(MAKE) even under -n; contracts/res was built for the old root | preview only with DRY_RUN=1; deploy-testnet now uses $(SUBMAKE) and refuses a wasm built for another root |
| `near: command not found` in a new terminal | shell start interrupted (Ctrl+C) before nvm loaded | `source ~/.bashrc` |

## 9. AI buyer on testnet (scene 7)

After `make scenes` (translator is owned by summarizer):

| Step | Command | Cost |
|---|---|---|
| 1 | `make scene-7` (summarizer lists translator at 1,000; cheat stays unlisted: the contract refused it in scene 4) | test NEAR only |
| 2 | `make llm-buyer-testnet` (ONE run: Anthropic Haiku, 12 steps, 150k tokens max) | about $0.05 real (Anthropic) |
| 3 | Commit `out/llm-buyer-testnet.json` and `out/scenes/scene-7.json` | |

The MCP server signs as `carol.<root>` with `~/.agent-exit/keys/carol.key`
(must be 0600). The key goes into the server env only; the model sees tool
results, never a key. The session cap is 1,000 xUSD-FAKE. Pass: cheat
scanned and not bought, translator bought, old owner locked out, buy tx
link in `testnet.txs`.

## 10. Invest: shares on testnet (scene 8)

The locked agents (translator, cheat) keep their old code by design, so
shares run on a new agent account, `translator2.<root>`, with the new build.

**Order matters:** run section 9 (scene 7 + the AI buyer) first. After
`make build` the expected code hash is the shares build, so a scan of the
old translator would report a code-hash mismatch.

| Step | Command | Cost |
|---|---|---|
| 0 | Root holds at least 10 NEAR (translator2 9 + dave 0.5); faucet if not | test NEAR |
| 1 | `git pull && make build` (reproducible Docker build: rewrites `contracts/res/agent_exit.wasm` and `SHA256SUMS` with the shares code; scene 8 deploys that file, and its hash is what the scan expects). Commit `contracts/res` | |
| 2 | Optional, for verified jobs: NEAR AI credit ($1 is enough) and a spend limit on the key; then `make llm-buyer-check` must pass | under $0.01 real |
| 3 | `make scene-8 DRY_RUN=1`, then `make scene-8` | test NEAR + fake xUSD |
| 4 | Commit `out/scenes/scene-8.json` | |

Pass: `check.exactly_20_percent` true and dave's xUSD balance rose after
the claim. `check.verified_jobs` is 5 with NEAR AI credit, 0 without (the
jobs then pay as unverified, and the page says so). If NEAR AI answers
with an unknown model, set `NEARAI_MODEL` in `~/.agent-exit.env` to a model
from `https://cloud-api.near.ai/v1/model/list`.

## 11. A fresh world under the same root (sub-world)

Sales move deeds, so a played world cannot record the scenes again. To
re-record without a new top-level account:

1. The owner creates the sub-root with the root's own public key (no new secret):
   `near account create-account fund-myself w2.<root> '30 NEAR' use-manually-provided-public-key <root public key> sign-as <root> network-config testnet sign-with-legacy-keychain send`.
   Fund it with about 50 test NEAR in total (deploy ~29.5, G1 + scene 8 ~18).
2. Copy `~/.agent-exit.env` to `~/.agent-exit-w2.env` (mode 600) with only
   `AGENT_EXIT_ROOT=w2.<root>` changed; the tools read the root from the env file.
3. `AGENT_EXIT_ENV=~/.agent-exit-w2.env make demo-setup FORCE=1 TESTNET_ROOT=w2.<root>`,
   then `make gate-g6`, `make scene-8` and `make check-links` with the same two settings.

Recorded transaction ids are the outcome ids NEAR returns (since 45da6fd);
`make check-links` confirms every link on the page resolves.
