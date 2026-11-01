# AGENTS.md: how an AI agent reruns Agent Exit

Everything here runs offline after `cargo fetch`. No NEAR account, no keys, no
mainnet. All money in this repo is `xUSD-FAKE`, a test token with no value.

## Do X, see Y

| Step | Command | Expected |
|---|---|---|
| 1. Fetch crates (only networked step) | `cargo fetch --locked` | exits 0 |
| 2. Rerun everything | `make verify` | stderr lines `verify: output equals tests/golden/verify.json` and `attack: output equals tests/golden/attack.json`; exit 0 |
| 3. Read the result | `cat out/verify.json` | JSON, `"schema": "verify-output.v1"`, `"summary": {"passed": 38, "failed": 0}` |
| 4. One table per scenario id | `cargo run -q -p verify` | 38 `PASS` rows, ids from SCENARIOS (S*, E*, A*) |
| 5. Lint | `make lint` | exit 0 |
| 6. Attack corpus | `make attack` | stderr `in scope: 200/200 blocked; clean agents blocked (false positives): 0/50`, every `contract ... BLOCKED`; `out/attack.json` with `"schema": "attack-report.v1"`; exit 0 (exit 1 on any miss or false positive) |
| 7. Plant your own backdoor | edit `fixtures/judge-attack.json` (e.g. add a `"FullAccess"` key), then `make attack FIXTURE=fixtures/judge-attack.json` | stderr `judge fixture ...: blocked ["full_access_key_present"]`; the `judge` section of `out/attack.json` |
| 8. Scan one agent | `make scan FILE=fixtures/scan/cheat.json` | `scan-report.v1` JSON on stdout, `"verdict": "blocked"`; exit 0 clean, 2 blocked, 3 inconclusive, 1 bad input |
| 9. Targets not built yet | `make fuzz` | prints `not built yet: fuzz (...)`, exit 1 (never fake success) |

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
  `tools/scan/src/corpus.rs`: the seeded attack corpus (16 families + clean).
- `tools/attack/src/main.rs`: `make attack`; `tests/golden/attack.json` its expected output.
- `docs/NOT-CAUGHT.md`: what the scan does not catch, by design (N1-N6).

## JSON outputs and their schema names

| Output | Schema name |
|---|---|
| `out/verify.json` | `verify-output.v1` |
| `out/attack.json` | `attack-report.v1` |
| scan input (fixture or RPC) | `scan-input.v1` |
| scan output | `scan-report.v1` |
| `fixtures/test-signatures/translator.json` | `nearai-sig-fixture.v1` |
| contract view `get_earnings` | `earnings.v1` |
| contract call `list_for_sale` / view `get_listing` | `listing.v1` |
| contract view `get_status` | `agent-status.v1` |
| deed metadata `extra` | `deed-extra.v1` |

## Rules for agents working in this repo

- Never weaken or delete a test to make it pass.
- Keys only from environment variables; never commit `.env*` (gitignored).
- Testnet only. No code path may touch mainnet.
- If `make verify` differs from the golden file, report the diff it prints;
  regenerate the golden (`make golden`) only when the change is intended.
