# AGENTS.md: how an AI agent reruns Agent Exit

Everything here runs offline after `make setup` (`cargo fetch` + `npm ci`). No NEAR account, no keys, no
mainnet. All money in this repo is `xUSD-FAKE`, a test token with no value.

## Judging or reviewing? Start here

Agent Exit is a capital market for AI agents on NEAR: invest in a share of an agent's earnings or buy the whole agent, with earnings counted by who paid and a one-transaction handover that locks the seller out. Three commands, offline:

| Command | Expected |
|---|---|
| `make setup && make mcp-build` (once; Rust + Node 20) | exit 0 |
| `make judge` (about 1 s once built; no cargo when `mcp/dist` is current; same as `node mcp/dist/scripts/judge-check.js`) | one PASS row per claim K1-K7, last line `judge: 7/7 claims pass (offline sandbox, real contract code, xUSD-FAKE)`, `out/check-claims.json`; exit 0 |
| `make screen` (1-3 min) | 7 rows, each with its promised result, `out/screen.json` |

- **Each claim -> its check -> its recorded transaction:** "Claims and evidence" in [`docs/PROOF.md`](docs/PROOF.md) (earnings, verified revenue, scan, refused buy, sale, lockout, invest, open offer in parts, MCP investor, sealed prompt, born clean, the Exchange, limits, AI buyer), with the world each ran on.
- **MCP:** Claude Code in this repo picks up [`.mcp.json`](.mcp.json): `agent-exit` (offline, you act as `summarizer.testnet`) and `agent-exit-testnet` (read-only, live `agentexit5.testnet`, needs RPC). Call `check_claims` or run the prompt `/mcp__agent-exit__judge_tour`. `check_claims` is K1-K7 offline and K1-K8 on testnet (K8 = gate G6, recorded on the previous world `agentexit4.testnet`); each testnet claim names its world.
- **No RPC?** A testnet claim then says `status: "unverified_network"`, `pass: null` (not checked, not failed), and `make judge-testnet` exits 3. The recorded transactions are in `out/scenes/*.json` (`txs[].explorer`); how to read them with curl: "Verify testnet without our server" in [`docs/PROOF.md`](docs/PROOF.md). The offline `check_claims` re-runs the contract code; it does not prove the testnet history.
- **Invest by hand (offline):** `list_offers` shows `research.testnet` offer 1 (20% open, smallest part 2%). Offline `buy_share` replays only the recorded buy: as `dave.testnet` (`AGENT_EXIT_ACCOUNT_ID=dave.testnet`), `buy_share {"agent":"research.testnet","offer_id":1,"pct":10,"max_price":150,"confirm":true}`, then `portfolio {"account":"dave.testnet"}`. Any other part: the `sandbox` tool (`offer_share`, `buy_share`, `claim_share`). On testnet the live offer on `translator3.agentexit5.testnet` sells parts of 2.5%.
- **Born clean and the Exchange** are testnet-only: offline `scan_agent` gives `born_clean: null`. With RPC, `scan_agent born1.factory.agentexit5.testnet` gives `born_clean: true`, and `list_agents` / `list_offers` show `translate-pro`, `research-desk` and `pump-bot` (`.factory.agentexit5.testnet`). Recorded: `out/scenes/scene-9.json`, `out/scenes/scene-13.json`.

## Do X, see Y

| Step | Command | Expected |
|---|---|---|
| 1. Fetch crates and npm packages (only networked step) | `make setup` | exits 0 |
| 2. Rerun everything | `make verify` | stderr lines `verify: output equals tests/golden/verify.json` and `attack: output equals tests/golden/attack.json`; exit 0 |
| 3. Read the result | `cat out/verify.json` | JSON, `"schema": "verify-output.v1"`, `"summary": {"passed": 73, "failed": 0}` |
| 4. One table per scenario id | `cargo run -q -p verify` | 73 `PASS` rows, ids from docs/SCENARIOS.md (S*, E*, A*) |
| 5. Lint | `make lint` | exit 0 |
| 6. Attack corpus | `make attack` | stderr summary `19 attack families, each blocked by its rule; 0/50 false positives (regression suite)`. The counts come from `tests/golden/attack.json`, in_scope 190/190: 20 attack families, 19 counted (A9 Base not counted until live); each blocked by its rule. It is a regression suite, not independent evidence; A5 checks the reproducible-build hash in `contracts/res/SHA256SUMS`. Every `contract ... BLOCKED`; `out/attack.json` with `"schema": "attack-report.v1"`; exit 0 (exit 1 on any miss or false positive) |
| 7. Plant your own backdoor | edit `fixtures/judge-attack.json` (e.g. add a `"FullAccess"` key), then `make attack FIXTURE=fixtures/judge-attack.json` | stderr `judge fixture ...: blocked ["full_access_key_present"]`; the `judge` section of `out/attack.json` |
| 8. Scan one agent | `make scan FILE=fixtures/scan/cheat.json` | `scan-report.v1` JSON on stdout, `"verdict": "blocked"`; exit 0 clean, 2 blocked, 3 inconclusive, 1 bad input |
| 9. MCP tests | `make mcp-test` | `# tests 185` (all pass), `# fail 0` (then 10 script tests and the near-key-check tests, each `# fail 0`), then `agent-buys-agent: output equals tests/golden/agent-buys-agent.json`; exit 0 |
| 10. Agent buys agent | `make agent-buys-agent` | a scripted replay, not an AI model; stderr one line per MCP call; `out/agent-buys-agent.json` with `"schema": "agent-buys-agent.v1"`, `"pass": true`, `summary.seller_locked_out_after: true` |
| 11. Demo page | `make web-test` (needs the `playwright` package with Chromium) | `web-test: N checks passed (desktop + 375px, offline + testnet flag)`; exit 0. `make web` serves it on port 8080 |
| 12. Fuzz | `make fuzz` (`FUZZ_CASES=3000`) | `test result: ok. 4 passed`; exit 0 |
| 13. Testnet plan, offline | `make deploy-testnet DRY_RUN=1`, `make gates DRY_RUN=1`, `make scenes DRY_RUN=1` | `DRY RUN (...): nothing is sent.` then numbered steps; exit 0 |
| 14. Targets not built yet | `make record` | prints `not built yet: record (...)`, exit 1 (never fake success); `make help` marks them `(not built yet)` |
| 15. The offline story | `make demo` | verify replay vs goldens, then the scripted agent-buys-agent replay (no AI model), then how to open the page; exit 0 |
| 16. Real model buyer | `make llm-buyer` | needs `NEARAI_API_KEY` in `~/.agent-exit.env`; exit 2 without it; writes `out/llm-buyer.json`; the model is the cheapest NEAR AI chat model that supports tools (else the default), logged with token usage. `make llm-buyer-check` first makes one tiny authenticated call (exit 2 on no key, 401, 402). a real model run (Claude Haiku 4.5) is committed as `out/llm-buyer.json` (offline fixtures) and `out/llm-buyer-testnet.json` (testnet) |
| 17. Pay an agent (customer) | call `storage_deposit` on the agent with 0.024 NEAR attached (once per account, A49), then `xusd.ft_transfer_call(agent, amount, msg)` with `msg` empty or `{"kind":"pay"}` | without `storage_deposit` the payment still counts but is pooled as `other`; `storage_balance_of(you)` shows whether you are registered; `storage_unregister` (1 yoctoNEAR) removes your own row if it never had a payment; `storage_withdraw` has nothing to return; `reclaim_payer_row(account)` removes a payment-less row after 30 days, refunding its depositor |
| 18. Translator worker | `make translator-serve` | listens on 127.0.0.1:8787 only (loopback; do not tunnel it without auth); offline serves a labelled fixture; real mode needs testnet + `NEARAI_API_KEY` + agent account, else exit 2 |
| 19. Claims, offline | `make judge` | `judge: 7/7 claims pass (offline sandbox, real contract code, xUSD-FAKE)`; exit 0 |
| 20. Claims, live testnet (read-only, needs RPC) | `make judge-testnet` | K1-K8, each `PASS` with its world; exit 0. Exit 1: a claim failed. Exit 3: no RPC answered, claims `NOT CHECKED (no RPC)` |
| 21. Who can take an account | `make check-account ID=trading-bot.testnet` (or MCP `check_account`) | `backdoor_risk` (a full-access key on a contract with an `upgrade` method); `locked-vault.testnet`: `locked` |
| 22. An agent invests (scripted, no model) | `make agent-invests` | reads open offers, picks by counted revenue per 1%, buys, reads its portfolio; `out/agent-invests.json` |
| 23. Recorded transactions exist | `make check-links` (needs RPC) | every transaction in `web/data/testnet.json` (built from `out/scenes/`) found on an archival node |

`make verify` takes about 2 minutes (10,000 property cases with a fixed seed).
Set `PROP_CASES=500 make verify` for a faster pass.

## Where things are

- `contracts/agent-core/src/lib.rs`: the state machine (listing, buy, handover,
  earn-out, runner guard). Every rule is here.
- `contracts/agent-core/tests/prop.rs`: property tests for invariants
  I1-I6, I9-I11 (random action sequences).
- `contracts/agent/src/lib.rs`: the NEAR contract wiring (NEP-141 receiver,
  NEP-171 deed, promises, callbacks).
- `contracts/agent/src/tests.rs`: unit tests named by scenario id
  (`s5_buy_one_step`, `e5_rollback`, `a23_hidden_key_freezes_and_returns_escrow`, ...).
- `contracts/agent/src/sim.rs`: the in-process simulator and the fixed world.
- `tools/verify/src/main.rs`: the rerun kit; `tests/golden/verify.json` its expected output.
- `fixtures/world.json`: the fixed world numbers.
- `contracts/agent/src/verified.rs`: NEAR Verified check (EIP-191 hash,
  `env::ecrecover`, admin-set signer list, agent/job binding, single use).
- `fixtures/test-signatures/translator.json`: TEST signatures made with a
  local key (`cargo run -q -p verify --bin gen-test-signatures -- --check`
  proves they regenerate byte for byte). Not NEAR AI Cloud signatures.
- `tools/scan/src/lib.rs`: the client backdoor scan rules;
  `tools/scan/src/corpus.rs`: the seeded attack corpus (20 attack families + clean controls).
- `tools/attack/src/main.rs`: `make attack`; `tests/golden/attack.json` its expected output.
- `web/`: the demo page (`make web`, `make web-test`); `web/data/demo.json` is recorded from the contract by `make web-data`.
- `docs/NOT-CAUGHT.md`: what the scan does not catch, by design (N1-N13; `make attack` prints N1-N6).

- `contracts/agent-core/src/policy.rs`: spending limits (feature 12): price
  floor, daily cap per (owner epoch, UTC day), allowed payees, revenue split.
- `mcp/src/tools.ts`: the thirteen MCP tools (`sealed_delivery` for the buyer side of a sealed config, six for buying, three for shares, `check_claims` for reviewers, `sandbox` for any action on the contract core compiled to wasm, `mcp/src/sandbox.ts`, and `check_account` for keys and code-change methods on any NEAR account); `mcp/src/backend.ts`: offline
  (recorded) and testnet (read-only RPC) data; `mcp/test/`: S14, S15, E29,
  E30 and the rest.
- `schemas/mcp/`: input and output JSON schemas for every tool.
- `skills/agent-exit/SKILL.md`: the Agent Skill (scan before you buy).
- `fixtures/mcp/world.json`: contract results recorded by
  `cargo run -q -p verify --bin gen-mcp-world` (`--check` proves it is current).

## Use the MCP server from Claude Code or Codex

Build once with `make mcp-build`. Then, from the repo root:

Claude Code reads [`.mcp.json`](.mcp.json) in the repo root (written by `node scripts/judge-mcp-config.mjs`; `--check` says if it is stale). It has two servers:

- `agent-exit`: `AGENT_EXIT_NETWORK=offline`, write tools on as `summarizer.testnet` (the recorded buyer); nothing is sent.
- `agent-exit-testnet`: `AGENT_EXIT_NETWORK=testnet`, read-only (no `AGENT_EXIT_ALLOW_WRITE`, no key), the agents of `agentexit5.testnet`: translator, translator3, cheat, born1 and the Exchange agents translate-pro, research-desk and pump-bot.

The smallest offline config: [`mcp/mcp-config.example.json`](mcp/mcp-config.example.json) (`{"AGENT_EXIT_NETWORK": "offline"}`).

Codex (`~/.codex/config.toml`):

```toml
[mcp_servers.agent-exit]
command = "node"
args = ["mcp/dist/src/server.js"]
env = { AGENT_EXIT_NETWORK = "offline" }
```

Environment: `AGENT_EXIT_NETWORK` = `offline` (default) or `testnet`
(read-only; anything else, any mainnet RPC or non-`.testnet` account makes
the server exit 2). Write tools (`buy_agent`, `list_for_sale`, `buy_share`) need
`AGENT_EXIT_ALLOW_WRITE=1` and `AGENT_EXIT_ACCOUNT_ID`; without them they
return `WRITE_DISABLED`. `buy_agent` also needs `AGENT_EXIT_RUNNER_PK`, the
public key it installs as the bought agent's runner key; it is never a tool
argument, so a model cannot be talked into handing the runner key to someone
else. `AGENT_EXIT_SESSION_CAP` (default 1500 dollars) caps what one session
may spend. `AGENT_EXIT_MIN_WINDOW_SECS` shortens the earn-out window
`buy_agent` accepts (demo only; outputs then say `terms_config: "demo"`).

Testnet only (each must be a `.testnet` account or an allowlisted testnet host):

- `AGENT_EXIT_AGENTS`: comma-separated agents that `list_agents` and `list_offers` show.
- `AGENT_EXIT_TOKEN`, `AGENT_EXIT_ADMIN`, `AGENT_EXIT_CODE_HASH`: the token, registry admin and agent code hash the scan expects (else it blocks with `registry_mismatch` or a code-hash finding).
- `AGENT_EXIT_RPC`, `AGENT_EXIT_RPC_2`: the two RPC providers the scan compares. `.mcp.json` uses two FastNEAR hosts (one operator, chosen because `rpc.testnet.near.org` rate-limits); `check_account` says so (`providers_same_operator`). For an independent cross-check set `AGENT_EXIT_RPC_2=https://rpc.testnet.near.org`.
- `AGENT_EXIT_FACTORY`: the born-clean factory; without it `scan_agent` returns `born_clean: null`.
- `AGENT_EXIT_MAX_SHARES_BPS` (default 2500): `buy_agent` refuses an agent with more of its earnings pledged to investors.

Signing keys only from env; the model API key (`NEARAI_API_KEY`) from a mode-600 env file (`~/.agent-exit.env`); a sealed config's one-time key in a 0600 file written by `buy_agent`, never shown to the model. Text that comes from the chain is in
fields named `untrusted_*`, sanitised and cut to 200 characters.

## JSON outputs and their schema names

| Output | Schema name |
|---|---|
| `out/verify.json` | `verify-output.v1` |
| `out/attack.json` | `attack-report.v1` |
| scan input (fixture or RPC) | `scan-input.v1` |
| scan output | `scan-report.v1` |
| `fixtures/test-signatures/translator.json` | `nearai-sig-fixture.v1` |
| contract call `set_limits` / view `get_limits` | `limits.v1` |
| MCP tool outputs | `agent-exit.mcp.<tool>.v1` (errors: `agent-exit.mcp.error.v1`), schemas in `schemas/mcp/` |
| `out/agent-buys-agent.json` | `agent-buys-agent.v1` |
| `fixtures/mcp/world.json` | `mcp-world.v1` |
| contract view `get_earnings` | `earnings.v1` |
| contract call `list_for_sale` / view `get_listing` | `listing.v1` |
| contract view `get_status` | `agent-status.v1` |
| deed metadata `extra` | `deed-extra.v1` |

## Gas per runner call

The runner signs with a function-call access key whose gas allowance is
5 NEAR (`RUNNER_ALLOWANCE`). Before each call the node charges the
*attached* gas against that allowance at a pessimistic gas price, so
attaching 300 TGas to every call can fail with `InvalidTransaction` even on
a fresh key. Attach only what the call needs:

| Runner call | Attach | Why |
|---|---|---|
| `heartbeat` | 30 TGas (`RUNNER_GAS_HEARTBEAT`) | state read and write only |
| `pay` | 100 TGas (`RUNNER_GAS_PAY`) | `ft_transfer` 10 TGas + callback 15 TGas, each plus the state term (70 Mgas per byte of the state blob; payer, day and signature rows are not in the blob, A47, so it stays a few KB), + execution |
| `pay_base` | 300 TGas (`RUNNER_GAS_PAY_BASE`) | the MPC `sign` call gets everything left after the callback's share |

Unused gas is refunded, and only burnt gas reduces the allowance: 5 NEAR
covers thousands of heartbeats. To refresh the allowance the owner rotates
the key with `set_runner_key` (1 yoctoNEAR), which adds the new key with a
fresh 5 NEAR allowance and deletes the old one.

## When a callback failed (recovery)

A callback that fails after its key batch ran (out of gas, say) leaves
state the contract can repair an hour later (`RECOVER_AFTER_NS`, A33):

| Stuck state | Who | Call | Result |
|---|---|---|---|
| `status: Settling` (buy callback never ran; the token refunded the buyer) | anyone | `recover_sale()` | seller keeps the deed, listing withdrawn, every key the buy touched deleted |
| `incoming_keys` / `retiring_keys` not empty (rotation or deletion callback never ran) | owner, 1 yoctoNEAR | `recover_keys()` | those keys deleted, confirmed runner keys kept |

Both delete each key in its own receipt; `on_keys_deleted` then clears
them. The owner sets a new runner key with `set_runner_key` afterwards.

## Rules for agents working in this repo

- Never weaken or delete a test to make it pass.
- Keys only from environment variables; never commit `.env*` (gitignored).
- Testnet only. No code path may touch mainnet.
- If `make verify` differs from the golden file, report the diff it prints;
  regenerate the golden (`make golden`) only when the change is intended.
