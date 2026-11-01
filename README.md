# Agent Exit

**Buy an AI agent with proven earnings and a proven handover, on NEAR.**

AI agents that earn money will be bought and sold. Today a buyer can trust
neither the numbers nor the handover: earnings can be faked by paying your own
agent, and a seller can keep a spare key and come back later. On NEAR an agent
can be its own account, so its keys, code and every payment it receives are
public. Agent Exit puts one contract on the agent's account that:

1. **counts earnings** by payer and day and flags payments from the seller,
   the seller's sub-accounts and round trips;
2. **hands the agent over in one transaction**: the ownership deed (an NFT)
   moves, the seller's runner key is deleted and the buyer's is added, all or
   nothing;
3. **pays the seller 70% now and holds 30% in escrow**, released only from
   real revenue after the sale. Silence pays a small floor; a proven backdoor
   cuts the rest;
4. **runs the agent as a business inside the owner's limits**: the agent has
   its own treasury, every incoming payment is split automatically, and the
   agent can pay other agents only to allowed payees and up to a daily cap.
   Spending policy enforced by the contract; buying the agent means taking
   over who sets the limits.

Other agents can do all of this through an MCP server (`mcp/`) and an Agent
Skill (`skills/agent-exit/SKILL.md`) that always scans before it buys.

> **All money here is fake.** Payments use `xUSD-FAKE`, a test token with no
> value that this repo deploys (`contracts/xusd`). Nothing runs on mainnet.

Status: hackathon build in progress (batches B1, B2 and B4 of the plan: contracts core, trust layer, business limits and MCP; B3, the testnet deploy, is next).
Every claim below says whether it is shown offline, in sandbox, or on testnet.

## Rerun it (humans and AI agents)

Requirements: Rust (the pinned toolchain in `rust-toolchain.toml` installs
itself through rustup) and `make`; Node >= 20 for the MCP server. No NEAR
account, no network after the first `make setup` (`cargo fetch` + `npm ci`).

| Do | See |
|---|---|
| `make verify` | Unit + property tests pass, then the fixed world replays through the real contract code and `out/verify.json` equals `tests/golden/verify.json`, and the attack report equals `tests/golden/attack.json` (`... output equals ...` lines on stderr). About 2 min, offline. |
| `cargo run -q -p verify` | A PASS/FAIL table, one row per scenario id (S*, E*, A*). |
| `cargo run -q -p verify -- --json` | The same as JSON (`verify-output.v1`). |
| `make attack` | 200 seeded backdoored agents (16 families) and 50 clean agents through the client scan, plus the contract's own refusals. Table on stderr, `out/attack.json` (`attack-report.v1`): `in scope: 200/200 blocked; ... false positives: 0/50`, and the "not caught by design" rows N1-N6. `N=`, `SEED=`, `CLEAN=` change the corpus. |
| `make attack FIXTURE=fixtures/judge-attack.json` | Scan **your own** planted agent: edit the fixture (instructions inside it), rerun, see its verdict and findings in the `judge` section. As committed it is clean. |
| `make scan FILE=fixtures/scan/cheat.json` | One scan report (`scan-report.v1`); exit 0 clean, 2 blocked, 3 inconclusive. The cheat agent is blocked for `full_access_key_present`. |
| `make mcp-test` | The MCP server's tests offline (18 tests: S14 agent buys agent, S15 resale, E29 refusals, E30 read-only without keys, never-mainnet, schemas, Skill), then the scripted run equals `tests/golden/agent-buys-agent.json`. |
| `make agent-buys-agent` | The `summarizer` agent, as an MCP client, lists agents, sees `cheat`'s earnings are fake, gets its buy refused (`SCAN_BLOCKED`), scans `translator` clean, buys it (`max_price` 1000, `confirm`), checks `alice` is locked out and lists the agent again. `out/agent-buys-agent.json` (`agent-buys-agent.v1`). |
| `make mcp` | Start the MCP server on stdio. Offline by default; write tools need `AGENT_EXIT_ALLOW_WRITE=1` and `AGENT_EXIT_ACCOUNT_ID`. Config for Claude Code and Codex: [`AGENTS.md`](AGENTS.md). |
| `make test` | All tests (`PROP_CASES=10000` property cases, fixed seed). |
| `make lint` | `rustfmt --check` and `clippy -D warnings`. |
| `make audit` | `cargo audit` (needs `cargo install cargo-audit`). Four advisories in near-sdk's native test runtime are ignored in `.cargo/audit.toml`; the same target proves those crates are not in the contract wasm. |
| `make test-sandbox` | Builds the wasm (`cargo-near`) and runs `tests/sandbox` on a real local node: key list before/after the buy, the old runner key and the deleted setup key failing with the node's error (S5, A17, A21), and a failed key batch refunding the buyer (E5). **Written and compiling; not yet run** (the cloud build box cannot raise the open-file limit near-sandbox needs). |
| `make help` | Every target. Targets for later batches print `not built yet` and exit 1. |

AI agents: start with [`AGENTS.md`](AGENTS.md) and [`llms.txt`](llms.txt).

## What is proven, and where

| Claim | How it is shown | Scenario ids |
|---|---|---|
| The meter separates real from faked earnings: the fixed world shows gross 500.00, counted 360.00, **verified 120.00**, flagged 140.00 (self 100.00, seller sub-account 40.00); a cheat agent with 500.00 gross counts 0.00 | offline (contract in mocked runtime) | S2, S4, A10, A11, A12 |
| NEAR Verified: a payment carrying a NEAR AI Cloud-style signature over `model:sha256(request):sha256(response)` is checked in the contract (EIP-191 hash, `env::ecrecover`, signer on an admin-set list, request names this agent and job, each signature once). A failed proof still counts, only without the badge | offline, **with TEST signatures** made by a local key (`fixtures/test-signatures/`, labelled; reproducible). The real NEAR AI signer is checked on the owner's machine (gate G6) | S3, E11-E14, A13, I9 |
| Backdoor scan: an extra full-access key, a key the contract did not add, a wider-scoped runner key, a wrong code hash, an upgrade or self-deploy method, an unpublished method, open approvals, an open Base authorization, a listing-version mismatch or a frozen agent each block the sale; RPC providers disagreeing makes it inconclusive (refuse) | offline, from JSON fixtures; `make attack` 200/200 caught, 0/50 false positives | S1 (client), A1-A9, A21, A23, E31, E34 |
| The contract keeps enforcing on its own: listing version and key hash at buy, approvals at listing, token origin, deed transfer, hidden keys | offline (`make attack` contract section) | A7, E2, A8, A14, A16, A23 |
| One `buy()` moves the deed, deletes the seller's runner key, adds the buyer's function-call key, pays 700.00 and escrows 300.00 | offline: the contract emits exactly that key batch and payout. Real key deletion on a node: `make test-sandbox` (written, not run yet) | S5, A17 |
| All-or-nothing: if the key batch fails, state is restored and the buyer is refunded in full; if the payout fails it becomes claimable | offline (callbacks driven with success and failure) | E5, E6, I1, I6 |
| Stale listing, changed keys, underpay, low gas, malformed msg, a second buy while settling: refused with a full refund | offline | E1-E4, E7, E10, A7 |
| Earn-out: 50% of each counted post-sale payment, floor 20% of escrow at window end, cap at escrow; wash payments release nothing | offline + property tests | S7-S10, E20, E21 |
| A self-call signed by a key the contract never added freezes the agent and returns the open escrow to the buyer | offline | A23 |
| Old seller after the sale: withdraw and deed transfer refused `NotOwner` | offline (contract part) | A19, A22 |
| The owner can always withdraw revenue and NEAR above the storage reserve | offline + property tests | E17, I10 |
| Spending policy enforced by the contract: the owner's one method `set_limits` sets price floor, daily cap, allowed payees and revenue split. A 25.00 job splits 20.00 owner / 5.00 treasury (the seller's share during the earn-out is the earn-out release, 12.50); the runner pays `summarizer` 8.00: allowed; 45.00 more: `PolicyViolation:DailyCap: 8.00 + 45.00 > daily cap 50.00`, balances unchanged | offline (contract in mocked runtime) | S11, S12 |
| Only the owner sets limits; the old seller and the runner get `NotOwner`; limits are refused while listed and move to the buyer; split != 100%, absurd caps, the agent as its own payee or negative amounts are refused | offline | S13, E33 |
| The cap counts per owner and UTC day: a seller burning the cap before the sale does not reduce the buyer's cap | offline | E18, E19, A15 |
| While listed, the limits are frozen and the runner cannot spend (`Listed: spending paused`); the listing snapshots the treasury and `buy()` refuses with a full refund (`TreasuryChanged`) if the treasury is below it, the same way as a changed listing version | offline + property test (treasury never below the snapshot while listed) | A24 |
| Agent buys agent through MCP: six tools, JSON schemas `schemas/mcp/*.v1.json`, read-only by default, `buy_agent` needs `confirm`, `max_price`, a session spend cap and a clean scan | offline: the server replays results **recorded from the real contract code** (`fixtures/mcp/world.json`, reproducible); nothing is sent. Testnet mode (`AGENT_EXIT_NETWORK=testnet`) is read-only, written but not run here | S14, S15 (MCP part), E29, E30, S17 |
| Invariants I1-I7, I9-I11 hold over random action sequences (I7: the runner never spends more than the daily cap; every payment's split adds up exactly; no spending while listed) | property tests, fixed seed (`contracts/agent-core/tests/prop.rs`) | I1-I7, I9-I11 |

Not built yet (later batches): the testnet deploy and live runs (B3),
testnet writes through MCP (they return `NOT_BUILT`), fuzz targets, Chain
Signatures Base wallet and `pay_base` (S6), `accept_job`, hosted page. Their
`make` targets say `not built yet`.

## How the handover works (prototype A, "locked account")

- The agent account has **no full-access key** after setup. The owner is
  whoever holds the deed (`nft_token("deed")`) and acts from their own account.
  The runner process holds one function-call key, added by the contract,
  limited to this account and the runner methods, with no deposit.
- A buyer calls `xusd.ft_transfer_call(agent, price, msg)` with
  `{"kind":"buy","listing_version":N,"known_keys_hash":H,"buyer_runner_pk":K}`.
  The contract checks the version and key hash the buyer scanned, locks the
  agent (`Settling`), and schedules **one receipt on its own account**:
  `delete_key(seller runner keys)` + `add_key(K, function-call)`.
- `on_handover` runs after that receipt. On success it moves the deed, bumps
  the owner epoch, pays the seller 70% (falls back to claimable if the push
  fails), escrows 30% and returns any overpay. On failure nothing changed and
  the full amount goes back to the buyer through `ft_resolve_transfer`.
- The key swap and the payout are separate receipts: the key swap is
  all-or-nothing and the seller is paid only if it succeeded; otherwise the
  buyer is refunded. The listing is locked during settlement.
- Moving the deed with plain `nft_transfer` runs the same key deletion and
  pauses the agent until the new owner sets a runner key.

## Honest limits

- The seller can keep copies of off-chain code, prompts, model weights and
  outside API keys. Nothing on-chain can take those back; the earn-out prices
  that risk in.
- Fake payers on aged or exchange-funded accounts look like real customers.
  The meter flags only what it can see (self, sub-accounts, round trips).
  Because the earn-out releases at most 50% of each payment, wash-paying
  always costs the seller money.
- A contract cannot list its own access keys. A hidden full-access key is
  caught by the buyer's scan before the sale, not by the contract; if a
  hidden key ever calls the contract, the contract freezes (A23). The full
  "not caught by design" list is [`docs/NOT-CAUGHT.md`](docs/NOT-CAUGHT.md).
- The scan's published code hash is a fixture value until the reproducible
  build hash exists (`contracts/res/SHA256SUMS`, needs Docker).
- NEAR Verified signatures in this repo are made with local test keys. The
  NEAR AI Cloud signer address is set by the registry admin (not by the
  seller, who could otherwise allowlist their own key) and is checked
  against the real service on the owner's machine.
- A failed supplier payment returns to the treasury but still counts toward
  that day's cap (conservative).
- The MCP server's offline mode replays recorded contract results; a buy or
  listing that was not recorded is refused (`OFFLINE_NO_RECORDING`), never
  made up. Testnet mode reads two RPC providers and refuses any mainnet URL
  or account; it has not run from this build box (no NEAR RPC access).
- NEAR Intents is mainnet-only and is not used.
- Mocked-runtime tests do not execute promises: they check the actions the
  contract schedules and drive callbacks by hand. Real `AccessKeyNotFound`
  errors and real token refunds need near-sandbox or testnet.
- The agent wasm is about 380 KB and not size-optimised yet.

## Repository map

```
contracts/agent-core/   pure state machine + money math + property tests (no NEAR runtime)
contracts/agent/        the NEAR contract (deed, meter, listing, buy, escrow) + simulator + unit tests
contracts/xusd/         FAKE test dollar (NEP-141), capped faucet
tools/verify/           offline rerun kit behind `make verify`; gen-test-signatures
tools/scan/             client backdoor scan (lib + `scan` CLI) and the seeded corpus
tools/attack/           `make attack`
mcp/                    MCP server (TypeScript, stdio) + tests + scripted agent-buys-agent
schemas/mcp/            JSON schemas v1 for every MCP tool input and output
skills/agent-exit/      Agent Skill: scan before you buy
fixtures/mcp/world.json recorded contract results the MCP offline mode replays
fixtures/judge-attack.json  edit me: plant your own backdoor
fixtures/test-signatures/   TEST signatures (local keys) for NEAR Verified
docs/NOT-CAUGHT.md      what is not caught, by design
tests/golden/           expected verify, attack and agent-buys-agent output
fixtures/world.json     the fixed world every test and the demo use
```

## Built with

- Rust, [near-sdk 5.30](https://crates.io/crates/near-sdk),
  near-contract-standards 5.30, borsh, serde, sha2, hex, bs58, proptest; k256 and
  sha3 only to generate test signatures;
  cargo-near 0.22 for wasm builds.
- TypeScript 5.9 on Node 22, `@modelcontextprotocol/sdk` 1.32, ajv 8.
- **AI use:** the code, tests and docs were written with Claude Code
  (Anthropic's Claude) as the coding agent, directed and reviewed by the
  project owner.

## License

MIT, see [LICENSE](LICENSE).
