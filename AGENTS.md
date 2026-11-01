# AGENTS.md: how an AI agent reruns Agent Exit

Everything here runs offline after `cargo fetch`. No NEAR account, no keys, no
mainnet. All money in this repo is `xUSD-FAKE`, a test token with no value.

## Do X, see Y

| Step | Command | Expected |
|---|---|---|
| 1. Fetch crates (only networked step) | `cargo fetch --locked` | exits 0 |
| 2. Rerun everything | `make verify` | last stderr line `verify: output equals tests/golden/verify.json`; exit 0 |
| 3. Read the result | `cat out/verify.json` | JSON, `"schema": "verify-output.v1"`, `"summary": {"passed": 38, "failed": 0}` |
| 4. One table per scenario id | `cargo run -q -p verify` | 38 `PASS` rows, ids from SCENARIOS (S*, E*, A*) |
| 5. Lint | `make lint` | exit 0 |
| 6. Targets not built yet | `make attack` | prints `not built yet: attack (...)`, exit 1 (never fake success) |

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

## JSON outputs and their schema names

| Output | Schema name |
|---|---|
| `out/verify.json` | `verify-output.v1` |
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
