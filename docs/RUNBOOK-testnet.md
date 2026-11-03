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
2. Fund it. The deploy needs **29 NEAR** (lean plan: 27.5 for the accounts + 1
   for gas); gate G1 needs **9 more** per run. Use near-faucet.io or the NEAR
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

Current build (2026-11-03, root `agentexit3.testnet`, STR-12 and STR-19 fixes):
agent_exit.wasm `5dc306a2c08322c2287eb2ae41d920e6ba0f0cf7c99950a2f202431fef9e371c`
(on-chain code hash `7K1Q5DWYuTKwbtUU8uvzrW4kyyFXG7ZVxvJDX7vAEWHq`), xusd.wasm
`f744bec2c1b4d3ba486624783530d596915a78fcd4a6e9298c72bc3bd7b0dde1`. The root is
compiled in, so another root gives other hashes. The first run (2026-11-02,
`agentexit.testnet`) was `be4b898d…` / `DoqGZ6zx…`, before the fixes.
`make build-testnet` gives different hashes (local toolchain); deploy and
scenes use contracts/res whenever SHA256SUMS exists.

The stale-build note of 2026-11-02 is resolved: the fixed build is deployed on `agentexit3.testnet`.

## 4. Deploy and seed

    make deploy-testnet DRY_RUN=1   # the plan, offline; nothing is sent
    make deploy-testnet             # create accounts, deploy, runner keys, lock translator, seed

Every step skips what is already done, so after any error fix the cause and
run the same command again. Afterwards `translator` and `cheat` each show
gross 500.00 (`get_earnings`), status Active, owner alice, solvent.
Output: `out/testnet/deploy.json`.

## 5. Gates

    make gate-g1     # key handover on a real node: about 9 NEAR (creates a fresh agent)
    make gate-g6     # NEAR AI signature on-chain: needs NEAR AI credit
    make gate-g2     # Chain Signatures on Base Sepolia: needs BASE_RELAYER_KEY or BASE_PAYEE
                     # then make testnet-setup && make gate-g2-relay

First run: G1 PASS; G6 failed with HTTP 402 (no NEAR AI credit, skipped);
G2 not run (no Base Sepolia key). Output: `out/gates/`.

## 6. The six scenes

    make scenes      # recording order 3 4 5 1 2 6 -> out/scenes/

| Scene | What it shows | First-run proof |
|---|---|---|
| 3 | Verified vs unverified revenue: `get_earnings` of translator (360.00 of 500.00 counted, 140.00 flagged) next to cheat (all 500.00 flagged as self-paid) | out/scenes/scene-3.json (views only, no tx) |
| 4 | Cheat caught: alice lists cheat at 500.00; the buyer's scan (keys from both RPCs + `get_status`) blocks it with `full_access_key_present`; no buy is sent | scene-4.json |
| 5 | The honest sale: bob buys `translator` in one transaction | [Auv99RhD…](https://testnet.nearblocks.io/txns/Auv99RhD4YmYkSNwKSGyf7p84GuqJMgWKiRBQY2EH3CF) |
| 1 | Break-in: the old seller is refused on-chain (withdraw, own key, re-list, `set_limits` NotOwner, deed move NotOwner, `pay_base` NotRunner); her old keys are rejected by the node | [set_limits refused](https://testnet.nearblocks.io/txns/A49Gt1m8ZYLqoewPumackPUt84w4zgnaaa2vrRnyb4VB) |
| 2 | Spending limits: cap 50.00, a pay of 8.00 passes, 45.00 is refused (PolicyViolation) | [refused pay](https://testnet.nearblocks.io/txns/4jL6cCWBK8TqWCZVkfLUz2SP7XNeEQ9nv2odsJpif7Ci) |
| 6 | An agent buys an agent: summarizer buys translator from bob | [summarizer buys](https://testnet.nearblocks.io/txns/26F5bi5ymjEYYz6AFqeS9EmRpj5gv9beehtBN3L11VbL) |

All outputs of the first run are committed in `out/testnet/`, `out/gates/` and `out/scenes/`.

## 7. Replay the demo

The scenes change the world (sales move the deed: after the first run
translator belongs to summarizer and cheat is listed). There is no reset
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

## 9. Open items

- Security review by the owner (planned, not started).
- G6 (NEAR AI credit) and G2 (Base Sepolia key) not run on testnet.
- `make demo-setup` builds a fresh world on a new root (section 7); a played root cannot be reset.
- Stress tests (STR-*) being written; run them on the sandbox when they land.
- Judge review 3 (2026-11-02): 41/60. Next, by points per effort: real AI buyer run (needs an API key, +2.5), page testnet links (+1.5), video (+1.5), G6 with NEAR AI credit (+1.5), scene 4 re-run on the fixed scan, hosted page.
- Stress run (2026-11-02, `make stress`): 26 scenarios, 22 pass. STR-12 (`withdraw` while Listed) and STR-19 (unbounded pending Base authorizations) are fixed in the contract code; they need `make build` and a fresh testnet deploy (`make demo-setup`). STR-17 (MCP spend cap across processes) is fixed.
- Real AI buyer: waiting for the Anthropic key; one capped run (12 steps, 150k tokens).

## 10. AI buyer on testnet (scene 7)

After `make scenes` (translator is owned by summarizer):

| Step | Command | Cost |
|---|---|---|
| 1 | `make scene-7` (summarizer lists translator at 1,000; cheat stays listed) | test NEAR only |
| 2 | `make llm-buyer-testnet` (ONE run: Anthropic Haiku, 12 steps, 150k tokens max) | about $0.05 real (Anthropic) |
| 3 | Commit `out/llm-buyer-testnet.json` and `out/scenes/scene-7.json` | |

The MCP server signs as `carol.<root>` with `~/.agent-exit/keys/carol.key`
(must be 0600). The key goes into the server env only; the model sees tool
results, never a key. The session cap is 1,000 xUSD-FAKE. Pass: cheat
scanned and not bought, translator bought, old owner locked out, buy tx
link in `testnet.txs`.

## 11. Invest: shares on testnet (scene 8)

The locked agents (translator, cheat) keep their old code by design, so
shares run on a new agent account, `translator2.<root>`, with the new build.

| Step | Command | Cost |
|---|---|---|
| 0 | Root holds at least 10 NEAR (translator2 9 + dave 0.5); faucet if not | test NEAR |
| 1 | `git pull && make build-testnet` (the shares build) | |
| 2 | Optional, for verified jobs: NEAR AI credit ($1 is enough) and a spend limit on the key; then `make llm-buyer-check` must pass | under $0.01 real |
| 3 | `make scene-8 DRY_RUN=1`, then `make scene-8` | test NEAR + fake xUSD |
| 4 | Commit `out/scenes/scene-8.json` | |

Pass: `check.exactly_20_percent` true and dave's xUSD balance rose after
the claim. `check.verified_jobs` is 5 with NEAR AI credit, 0 without (the
jobs then pay as unverified, and the page says so). If NEAR AI answers
with an unknown model, set `NEARAI_MODEL` in `~/.agent-exit.env` to a model
from `https://cloud-api.near.ai/v1/model/list`.
