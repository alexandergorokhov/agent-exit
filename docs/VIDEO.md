# Demo videos

Two videos, each 3:00 or less. The break-in comes first, inside the first 90 seconds.
All money shown is `xUSD-FAKE`; say so once, on screen.

## Before recording

1. Testnet world is deployed and seeded: `make deploy-testnet` (on the owner's PC).
2. Record the scenes in **recording order** 3, 4, 5, 1, 2, 6 (the sale must happen before the break-in): `make scenes`. Each writes `out/scenes/scene-N.json` with explorer links.
3. Replay any scene offline for a clean take: `make scene-N RECORDED=1`.
4. Open in tabs: the demo page (`make web`, or the hosted URL), testnet.nearblocks.io for `translator.agentexit3.testnet`, a terminal at the repo root with a large font.
5. Recorder: OBS or the Windows Game Bar (Win+Alt+R), 1920x1080, mic on. No edits needed beyond cutting between scenes.

## Video 1: 3-minute pitch

Edited in this order (not the recording order).

| Time | Beat | Screen | Say (plain, about 2.5 words per second) |
|---|---|---|---|
| 0:00-0:12 | Hook | Page title + "All money here is fake" | "AI agents that earn money will be bought and sold. The buyer has two questions: are the earnings real, and is the seller really gone? Agent Exit answers both on NEAR." |
| 0:12-1:20 | **Break-in fails** (scene 1) | Page step 1, then NearBlocks: key list before and after | "This agent was just sold. Here is its key list before the sale and after: the seller's key is gone, the buyer's key is in, in one transaction. Now the old seller tries to get back in. Withdraw the treasury: refused. Set a new key: refused. Move the deed: refused. Use the old key directly: the node itself says the key does not exist. Every one of these is a real testnet transaction you can open." |
| 1:20-1:40 | Business inside limits (scene 2) | Page step 2 | "The new owner sets spending limits. A job pays 25, split 20 to the agent and 5 to the owner. The agent pays another agent 8. Then it tries 45, and the contract refuses: daily cap. Buying the agent means taking over who sets those limits." |
| 1:40-2:00 | Verified vs unverified (scene 3) | Page step 3 | "Two agents both claim 500 in earnings. Ours counts every payment by payer and flags the seller paying himself. Work signed by NEAR AI Cloud gets a Verified badge. One agent: 500 gross, 360 counted. The other: 500 gross, nothing counted." |
| 2:00-2:20 | Cheat caught (scene 4) | Page step 4 + `make attack` table | "This one is a cheat: the seller kept a hidden full-access key. The scan finds it and the sale is blocked before any money moves. The attack suite runs 20 attack families, with zero false positives on 50 honest agents." |
| 2:20-2:40 | Honest sale (scene 5) | NearBlocks: the buy tx | "An honest sale is one transaction: deed, keys and wallet move together. The seller gets 70 percent now. 30 percent stays in escrow and is paid only from the agent's real future earnings." |
| 2:40-3:00 | Agent buys agent (scene 6) + close | Terminal: `make llm-buyer` transcript, then README | "And the buyer can be an agent. Here a model on NEAR AI Cloud reads our skill, refuses the cheat, buys the honest agent and checks the seller is locked out. Everything is open source. Run make verify to check it yourself." |

If the real AI-buyer run (`out/llm-buyer.json`) does not exist, use page step 6 and say "a scripted replay" instead of "a model".

## Video 2: tech walkthrough (2-3 minutes)

| Time | Beat | Screen |
|---|---|---|
| 0:00-0:25 | Architecture: agent account = agent contract; deed is NEP-171; payments in NEP-141 `xUSD-FAKE` via `ft_transfer_call` | README sequence diagram |
| 0:25-0:55 | The handover: one batch on the agent's own account deletes the seller's runner key and adds the buyer's, then `on_handover` moves the deed; a failed batch refunds | `contracts/agent/src/lib.rs` buy path |
| 0:55-1:20 | Why NEAR: deletable and scoped keys; Chain Signatures Base wallet derived from the account name; NEAR AI signatures checked on-chain with `ecrecover` | README "Why only on NEAR" |
| 1:20-1:50 | Found on a real node: the 585-payer brick (A47), how state was split into rows, the sandbox test with 1,000 payers | README Evidence, `tests/sandbox` |
| 1:50-2:20 | For agents: MCP server with 6 tools, spend caps, allowlist, untrusted chain text | `mcp/`, `AGENTS.md` |
| 2:20-2:45 | Verify it: `make verify`, `make attack`; `make build` reproduces `contracts/res/SHA256SUMS` (agent_exit.wasm `5dc306a2…`), the same code hash NearBlocks shows for the deployed agent | Terminal |

## Rules

- No claim in a video that the README does not back with a test or a transaction.
- Say "fake test dollars" once; never show a mainnet account.
- Upload unlisted; the link goes in README line 3 and the submission.
