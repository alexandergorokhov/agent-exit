# AGENTS.md: how an AI agent reruns Agent Exit

Everything here runs offline after `make setup` (`cargo fetch` + `npm ci`). No NEAR account, no keys, no
mainnet. All money in this repo is `xUSD-FAKE`, a test token with no value.

## Do X, see Y

| Step | Command | Expected |
|---|---|---|
| 1. Fetch crates and npm packages (only networked step) | `make setup` | exits 0 |
| 2. Rerun everything | `make verify` | stderr lines `verify: output equals tests/golden/verify.json` and `attack: output equals tests/golden/attack.json`; exit 0 |
| 3. Read the result | `cat out/verify.json` | JSON, `"schema": "verify-output.v1"`, `"summary": {"passed": 53, "failed": 0}` |
| 4. One table per scenario id | `cargo run -q -p verify` | 53 `PASS` rows, ids from SCENARIOS (S*, E*, A*) |
| 5. Lint | `make lint` | exit 0 |
| 6. Attack corpus | `make attack` | stderr `in scope: 200/200 blocked; clean agents blocked (false positives): 0/50`, every `contract ... BLOCKED`; `out/attack.json` with `"schema": "attack-report.v1"`; exit 0 (exit 1 on any miss or false positive) |
| 7. Plant your own backdoor | edit `fixtures/judge-attack.json` (e.g. add a `"FullAccess"` key), then `make attack FIXTURE=fixtures/judge-attack.json` | stderr `judge fixture ...: blocked ["full_access_key_present"]`; the `judge` section of `out/attack.json` |
| 8. Scan one agent | `make scan FILE=fixtures/scan/cheat.json` | `scan-report.v1` JSON on stdout, `"verdict": "blocked"`; exit 0 clean, 2 blocked, 3 inconclusive, 1 bad input |
| 9. MCP tests | `make mcp-test` | `# pass 18`, `# fail 0`, then `agent-buys-agent: output equals tests/golden/agent-buys-agent.json`; exit 0 |
| 10. Agent buys agent | `make agent-buys-agent` | stderr one line per MCP call; `out/agent-buys-agent.json` with `"schema": "agent-buys-agent.v1"`, `"pass": true`, `summary.seller_locked_out_after: true` |
| 11. Targets not built yet | `make fuzz` | prints `not built yet: fuzz (...)`, exit 1 (never fake success) |

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

- `contracts/agent-core/src/policy.rs`: spending limits (feature 12): price
  floor, daily cap per (owner epoch, UTC day), allowed payees, revenue split.
- `mcp/src/tools.ts`: the six MCP tools; `mcp/src/backend.ts`: offline
  (recorded) and testnet (read-only RPC) data; `mcp/test/`: S14, S15, E29,
  E30 and the rest.
- `schemas/mcp/`: input and output JSON schemas for every tool.
- `skills/agent-exit/SKILL.md`: the Agent Skill (scan before you buy).
- `fixtures/mcp/world.json`: contract results recorded by
  `cargo run -q -p verify --bin gen-mcp-world` (`--check` proves it is current).

## Use the MCP server from Claude Code or Codex

Build once with `make mcp-build`. Then, from the repo root:

Claude Code (`.mcp.json` in the repo root, or `claude mcp add`):

```json
{
  "mcpServers": {
    "agent-exit": {
      "command": "node",
      "args": ["mcp/dist/src/server.js"],
      "env": { "AGENT_EXIT_NETWORK": "offline" }
    }
  }
}
```

Same file: [`mcp/mcp-config.example.json`](mcp/mcp-config.example.json).

Codex (`~/.codex/config.toml`):

```toml
[mcp_servers.agent-exit]
command = "node"
args = ["mcp/dist/src/server.js"]
env = { AGENT_EXIT_NETWORK = "offline" }
```

Environment: `AGENT_EXIT_NETWORK` = `offline` (default) or `testnet`
(read-only; anything else, any mainnet RPC or non-`.testnet` account makes
the server exit 2). Write tools (`buy_agent`, `list_for_sale`) need
`AGENT_EXIT_ALLOW_WRITE=1` and `AGENT_EXIT_ACCOUNT_ID`; without them they
return `WRITE_DISABLED`. `AGENT_EXIT_SESSION_CAP` (default 1500 dollars)
caps what one session may spend. Keys only from env, never from files.

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
| `pay` | 60 TGas (`RUNNER_GAS_PAY`) | `ft_transfer` 10 TGas + callback 10 TGas + execution |

Unused gas is refunded, and only burnt gas reduces the allowance: 5 NEAR
covers thousands of heartbeats. To refresh the allowance the owner rotates
the key with `set_runner_key` (1 yoctoNEAR), which adds the new key with a
fresh 5 NEAR allowance and deletes the old one.

## Rules for agents working in this repo

- Never weaken or delete a test to make it pass.
- Keys only from environment variables; never commit `.env*` (gitignored).
- Testnet only. No code path may touch mainnet.
- If `make verify` differs from the golden file, report the diff it prints;
  regenerate the golden (`make golden`) only when the change is intended.
