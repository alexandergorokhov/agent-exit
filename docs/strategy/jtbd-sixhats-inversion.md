# Positioning and submission check: Jobs to Be Done, Six Hats, Inversion

2026-11-11, against main 9602481. Thinking tools only; nothing here was run on a chain.

## Jobs (no product named)

- **Investor:** *When I see AI agents earning real money, I want to own a slice of an agent's income without trusting whoever runs it, so I can earn from the AI economy itself, not from tokens about it.* (Emotional: early but not a mark. Social: backs real revenue, not hype.)
- **Buyer:** *When I find an agent that already has customers, I want to take it over knowing the seller can't take it back or have faked its history, so I can run it as my business from day one.* (Emotional: safe paying a stranger. Social: not the one who got rugged.)

Forces: the barrier is **anxiety** (faked revenue, a kept key, a rug), not lack of pull. Everything built removes that anxiety, yet the first line sells the pull ("a capital market"). Real competition: AI-agent *tokens* with no revenue claim, private deals on trust, building your own, and doing nothing.

## Copy

- **Page lede (2 sentences):** *AI agents earn money, but their revenue can be faked and their sellers can keep a key. Agent Exit checks both on NEAR, so you can buy a share of an agent's earnings, or the whole agent, safely.*
- **X line:** *AI agent tokens are bets on hype. This is a share of an agent's actual revenue, checked on-chain: invest 2.5% on testnet in 2 minutes.*

## Inversion: how to guarantee a judge skips Agent Exit

| Guaranteed-skip move | Mechanism | Status on main |
|---|---|---|
| Open with a category word ("capital market") instead of the fear | A judge can't tell in 5 s why this beats agent tokens | still the hero's first words |
| Make every user ours | "Would anyone use it?" goes unanswered, so Impact reads as a prototype | true: one scripted outside account |
| Bury the 60-90 s story under 13 scenes | A human gives a project minutes; depth never reached is worth zero | improved (nav + hero), video length unknown |
| Leave a red check for the AI screener | The screener's report leads with the failure | fixed (lint, judge 8/8, links 70/70) |
| Say "testnet only" with no path to money | VC judges ask "who pays, when?" | the 1% fee is in BUSINESS.md, not on the first screen |
| Spread across 4 tracks with one generic pitch | Each track judge sees no answer to their track's ask | tracks are listed, not answered per track |

Dead flips (checked, no live case): "judges reward less proof" (no; the proof is an asset once the story lands); "drop MCP / AI buyer" (no; Agent Economy rewards it).

## The 10 fixes, ranked by judge impact

| # | Fix | File(s) |
|---|---|---|
| 1 | Hero lede leads with the fear removed (the 2-sentence lede above); keep "capital market" as the second line, not the first | `web/index.html` (`header .lede`; F1 test allows 2 sentences) |
| 2 | Video 60-90 s, opening within 20 s on a backdoor caught and the seller locked out; poster frame on the hero | `web/media/agent-exit-demo.mp4`, `web/index.html`, `docs/VIDEO.md` |
| 3 | Live "outside investors" counter next to Invest (holders of translator3 that aren't ours), plus the scripted `p6` purchase as the first example | `web/app.js` (via relay `get_shares`), `web/data/testnet.json` (`open_offer.example_tx` exists) |
| 4 | Get 10-30 real outside testnet investors before the deadline (X with @NEARProtocol and #NEARHackathon, NEAR builder chats) | owner action; post text: X line above |
| 5 | Mainnet-account guard: typing `*.near` in Invest explains "use a .testnet account" with a link to testnet.mynearwallet.com (a real user hit this) | `web/app.js` (`wireWallet` `update`) |
| 6 | One line "Who pays: 1% of every sale and share buy" in the hero or under the Exchange, and the README's first lines | `web/index.html`, `README.md` |
| 7 | Per-track one-liners answering each track's ask (Autonomous Business, AI Security, NFTs That Do Something, Agent Economy) in the judges section and the submission text | `web/index.html` (`#for-judges`), `docs/PITCH.md` |
| 8 | Submission text ("What did you build?", 2,000 chars): the investor job sentence first, then 3 proofs, `make judge-testnet`, the 4 tracks | new `docs/SUBMISSION.md` (draft for Alex) |
| 9 | README first screen mirrors the new lede, with the two verify commands right under it (`make judge`, `make judge-testnet`) | `README.md` lines 1-15 |
| 10 | Code freeze Nov 13 23:59 UTC, then one clean-clone judge pass on the PC (judge, judge-testnet, verify, check-links, web-test) before Alex submits | process; `docs/OPERATIONS.md` checklist |

**Confidence:** medium-high on fixes 1-3 and 5 (they change what a judge sees in the first minute). Fix 4 is the largest Impact lever but depends on reach. The open tension: the depth of proof is the edge only if the first screen lands in about 10 seconds.
