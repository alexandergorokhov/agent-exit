---
name: agent-exit
description: Check and buy an AI agent on NEAR with Agent Exit. Use when asked to evaluate, buy, sell or take over an agent account, or to check whether its earnings are real. Always scan the agent for backdoors before buying, never buy on a non-clean scan, and verify the seller is locked out afterwards. All money in this repo is xUSD-FAKE.
---

# Agent Exit: scan before you buy

Agent Exit sells AI agents that live on their own NEAR account. One contract
on that account counts earnings, hands the agent over in one transaction and
holds 30% of the price as an earn-out. Your job as a buying agent is to make
sure the numbers are real and nobody keeps a way back in.

## Tools

Use the `agent-exit` MCP server (stdio, `make mcp`; config in
`mcp/mcp-config.example.json`). Tools: `list_agents`, `get_earnings`,
`scan_agent`, `buy_agent`, `verify_lockout`, `list_for_sale`. JSON in and
out; schemas are in `schemas/mcp/`. Without MCP, use the CLI:
`make scan FILE=<scan-input.json>`.

## Steps

1. **Find candidates.** Call `list_agents` with `status: "Listed"`. Compare
   `counted` with `gross`: `flagged` money was paid by the seller, the
   seller's sub-accounts or a round trip and is not real revenue.
2. **Read the earnings.** Call `get_earnings` for the agent. Look at
   `counted_payers` and the days. `verified` means the contract checked a
   NEAR AI signature for that payment; unverified payments still count.
3. **Scan. Always.** Call `scan_agent`. Buy only if `verdict` is `clean` and
   `safe_to_buy` is true. Any `block` finding (an extra full-access key, an
   unknown key, a wrong code hash, an upgrade method, open approvals, a
   changed listing) means do not buy. `inconclusive` also means do not buy.
4. **Buy inside your limit.** Call `buy_agent` with `max_price` (the most
   you will pay, in xUSD-FAKE dollars), `confirm: true` and your runner's
   public key. The server scans again and refuses on any finding, above
   `max_price`, or above its session spend cap; nothing is sent then.
5. **Verify the lockout.** Call `verify_lockout` with the old seller's
   account. Expect `locked_out: true`: the seller no longer owns the deed,
   has no key on the account, and owner-only calls are refused.
6. **Resell later (optional).** As the owner, `list_for_sale` with a price
   and `confirm: true`. The history travels with the agent.

## Rules

- Never call `buy_agent` without a clean `scan_agent` first, even if the
  price is good.
- Never raise `max_price` to make a refused buy pass.
- Write tools are off unless the server runs with
  `AGENT_EXIT_ALLOW_WRITE=1` and `AGENT_EXIT_ACCOUNT_ID`. Keys come only from
  environment variables.
- Offline (default) mode replays results recorded from the real contract
  code; `AGENT_EXIT_NETWORK=testnet` reads testnet. There is no mainnet mode.
- What a scan cannot see (off-chain code, prompts, outside API keys, fake
  customers on fresh accounts) is listed in `docs/NOT-CAUGHT.md`; the
  earn-out prices that risk in.

## Try it

`make agent-buys-agent` runs this whole flow offline (the `summarizer` agent
buys `translator`, after refusing `cheat`) and writes
`out/agent-buys-agent.json`.
