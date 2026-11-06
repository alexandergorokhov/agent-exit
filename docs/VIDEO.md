# Demo videos

Two videos, each 3:00 or less. The break-in comes inside the first 90 seconds.
All money shown is `xUSD-FAKE`; say so once, on screen.

## Before recording

1. Testnet world is deployed and seeded: `make deploy-testnet` (on the owner's PC).
2. Record the scenes in **recording order** 3, 4, 5, 1, 2, 6 (the sale must happen before the break-in): `make scenes`. Each writes `out/scenes/scene-N.json` with explorer links.
3. Replay any scene offline for a clean take: `make scene-N RECORDED=1`.
4. Open in tabs: the demo page (`make web`, or https://near-transparent-agent-market-web.vercel.app/?network=testnet), testnet.nearblocks.io for `translator.w2.agentexit3.testnet`, a terminal at the repo root with a large font.
5. Recorder: OBS or the Windows Game Bar (Win+Alt+R), 1920x1080, mic on. No edits needed beyond cutting between scenes.

## Video 1: 3-minute pitch

Story order (owner's choice, 2026-11-04): **business, then proven earnings, then what they unlock (invest or buy), then security**.
The break-in still starts inside the first 90 seconds. Edited in this order (not the recording order).

| Time | Beat | Screen | Say (plain, about 2.5 words per second) |
|---|---|---|---|
| 0:00-0:12 | Hook | Page title + "All money here is fake" | "AI agents are becoming small businesses: they do jobs and get paid. Agent Exit turns one into a business you can trust: earnings proven on-chain, shares you can invest in, and a safe exit." |
| 0:12-0:35 | **Earnings proven** (scene 3) | Page step 3, then playground mission 1 | "The agent is its own NEAR account, and its contract counts every payment by who paid. Payments from the owner or the owner's own accounts are flagged, not counted. Two agents both show 500: one has 360 counted, the other zero. Work signed by NEAR AI Cloud is checked on-chain and marked verified." |
| 0:35-0:55 | **Invest in a share** (scene 8) | Page scenario "invest", NearBlocks claim tx | "Proven earnings unlock money. Dave buys 20 percent of this agent's future earnings. A customer pays for a NEAR AI translation, and Dave claims exactly 20 percent. A payment the owner makes to himself is flagged and releases nothing." |
| 0:55-1:05 | Cheat caught (scene 4) | Page step 4 | "Or buy the whole agent. First the buyer scans it: this seller kept a hidden full-access key, so the sale is blocked before any money moves." |
| 1:05-1:20 | Honest sale (scene 5) | Page proof strip, then NearBlocks: the buy tx | "An honest sale is one transaction: the deed moves, the seller's key is deleted, the buyer's key is added. The seller gets 70 percent now; 30 percent is paid only from real customer payments after the sale." |
| 1:20-1:55 | **The seller is locked out** (scene 1) | Page proof strip: the 6 refused calls, open two on NearBlocks | "Now the old seller tries to get back in. Withdraw the money: refused. Add her own key: refused. List it for sale: refused. Move the deed: refused. Spend from the Base wallet: refused. Her old key no longer exists. Every line is a real testnet transaction." |
| 1:55-2:15 | Business inside limits (scene 2) | Page step 2 | "The new owner sets limits. Customers pay, the money is split automatically, and the agent pays another agent 8. It tries 45, and the contract refuses: daily cap." |
| 2:15-2:45 | Agent buys agent (scene 7) | Page scenario "aibuyer" | "The buyer can be an AI agent too. Claude Haiku, with only our skill and MCP tools, refused the cheat, bought the honest agent on testnet, and checked the seller is locked out, for three cents." |
| 2:45-3:00 | Close | README top | "Open source, MIT. Press Run and verify on the page, or run make verify, to check every step yourself." |

If the real AI-buyer run (`out/llm-buyer-testnet.json`) does not exist, use page step 6 and say "a scripted replay" instead of "a model".

## Video 2: tech walkthrough (2-3 minutes)

| Time | Beat | Screen |
|---|---|---|
| 0:00-0:25 | Architecture: agent account = agent contract; deed is NEP-171; payments in NEP-141 `xUSD-FAKE` via `ft_transfer_call` | README sequence diagram |
| 0:25-0:55 | The handover: one batch on the agent's own account deletes the seller's runner key and adds the buyer's, then `on_handover` moves the deed; a failed batch refunds | `contracts/agent/src/lib.rs` buy path |
| 0:55-1:20 | Why NEAR: deletable and scoped keys; Chain Signatures Base wallet derived from the account name; NEAR AI signatures checked on-chain with `ecrecover` | README "Why only on NEAR" |
| 1:20-1:50 | Found on a real node: the 585-payer brick (A47), how state was split into rows, the sandbox test with 1,000 payers | README Evidence, `tests/sandbox` |
| 1:50-2:20 | For agents: MCP server with 10 tools (incl. shares and check_claims), spend caps, allowlist, untrusted chain text | `mcp/`, `AGENTS.md` |
| 2:20-2:45 | Verify it: `make verify`, `make attack`; `make build` reproduces `contracts/res/SHA256SUMS` (agent_exit.wasm `91bc8a84…`, code hash `Aots3gb3…`), the same code hash NearBlocks shows for the deployed agent | Terminal |

## Rules

- No claim in a video that the README does not back with a test or a transaction.
- Say "fake test dollars" once; never show a mainnet account.
- Upload unlisted; the link goes in README line 3 and the submission.
