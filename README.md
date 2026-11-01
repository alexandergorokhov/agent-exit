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
   cuts the rest.

> **All money here is fake.** Payments use `xUSD-FAKE`, a test token with no
> value that this repo deploys (`contracts/xusd`). Nothing runs on mainnet.

Status: hackathon build in progress (batch B1 of the plan: contracts core).
Every claim below says whether it is shown offline, in sandbox, or on testnet.

## Rerun it (humans and AI agents)

Requirements: Rust (the pinned toolchain in `rust-toolchain.toml` installs
itself through rustup) and `make`. No NEAR account, no network after the first
`cargo fetch`.

| Do | See |
|---|---|
| `make verify` | Unit + property tests pass, then the fixed world replays through the real contract code and `out/verify.json` equals `tests/golden/verify.json` (last line: `verify: output equals tests/golden/verify.json`). About 2 min, offline. |
| `cargo run -q -p verify` | A PASS/FAIL table, one row per scenario id (S*, E*, A*). |
| `cargo run -q -p verify -- --json` | The same as JSON (`verify-output.v1`). |
| `make test` | All tests (`PROP_CASES=10000` property cases, fixed seed). |
| `make lint` | `rustfmt --check` and `clippy -D warnings`. |
| `make audit` | `cargo audit` (needs `cargo install cargo-audit`). Four advisories in near-sdk's native test runtime are ignored in `.cargo/audit.toml`; the same target proves those crates are not in the contract wasm. |
| `make help` | Every target. Targets for later batches print `not built yet` and exit 1. |

AI agents: start with [`AGENTS.md`](AGENTS.md) and [`llms.txt`](llms.txt).

## What is proven, and where

| Claim | How it is shown | Scenario ids |
|---|---|---|
| The meter separates real from faked earnings: the fixed world shows gross 500.00, counted 360.00, flagged 140.00 (self 100.00, seller sub-account 40.00); a cheat agent with 500.00 gross counts 0.00 | offline (contract in mocked runtime) | S2, S4, A10, A11, A12 |
| One `buy()` moves the deed, deletes the seller's runner key, adds the buyer's function-call key, pays 700.00 and escrows 300.00 | offline: the contract emits exactly that key batch and payout. **Real key deletion on a node: sandbox/testnet, not run yet** | S5, A17 |
| All-or-nothing: if the key batch fails, state is restored and the buyer is refunded in full; if the payout fails it becomes claimable | offline (callbacks driven with success and failure) | E5, E6, I1, I6 |
| Stale listing, changed keys, underpay, low gas, malformed msg, a second buy while settling: refused with a full refund | offline | E1-E4, E7, E10, A7 |
| Earn-out: 50% of each counted post-sale payment, floor 20% of escrow at window end, cap at escrow; wash payments release nothing | offline + property tests | S7-S10, E20, E21 |
| A self-call signed by a key the contract never added freezes the agent and returns the open escrow to the buyer | offline | A23 |
| Old seller after the sale: withdraw and deed transfer refused `NotOwner` | offline (contract part) | A19, A22 |
| The owner can always withdraw revenue and NEAR above the storage reserve | offline + property tests | E17, I10 |
| Invariants I1-I6, I9-I11 hold over random action sequences | property tests, fixed seed (`contracts/agent-core/tests/prop.rs`) | I1-I6, I9-I11 |

Not built yet (later batches): NEAR Verified badge (S3; so the fixed world's
120.00 "verified" is not claimed yet), client backdoor scan and attack corpus
(A1-A6), Chain Signatures Base wallet (S6), spending policy (S11-S13), MCP
server, hosted page. Their `make` targets say `not built yet`.

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
  caught by the buyer's scan before the sale (batch B2), not by the contract;
  if a hidden key ever calls the contract, the contract freezes (A23).
- Mocked-runtime tests do not execute promises: they check the actions the
  contract schedules and drive callbacks by hand. Real `AccessKeyNotFound`
  errors and real token refunds need near-sandbox or testnet.
- The agent wasm is about 380 KB and not size-optimised yet.

## Repository map

```
contracts/agent-core/   pure state machine + money math + property tests (no NEAR runtime)
contracts/agent/        the NEAR contract (deed, meter, listing, buy, escrow) + simulator + unit tests
contracts/xusd/         FAKE test dollar (NEP-141), capped faucet
tools/verify/           offline rerun kit behind `make verify`
tests/golden/           expected verify output
fixtures/world.json     the fixed world every test and the demo use
```

## Built with

- Rust, [near-sdk 5.30](https://crates.io/crates/near-sdk),
  near-contract-standards 5.30, borsh, serde, sha2, hex, proptest;
  cargo-near 0.22 for wasm builds.
- **AI use:** the code, tests and docs were written with Claude Code
  (Anthropic's Claude) as the coding agent, directed and reviewed by the
  project owner.

## License

MIT, see [LICENSE](LICENSE).
