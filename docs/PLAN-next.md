# Next plan: A, B, C and the hosted page (saved 2026-11-03)

Judge review 5: 46/60. This plan targets about 51-52/60. Run it from any
session: the cloud can do every "Cloud" step; "PC" steps need Alex's machine
(WSL, `~/src/near-transparent-agent-market`, `~/.agent-exit.env`); "Alex"
steps need his logins. Never trigger GitHub Actions. Testnet root:
`agentexit3.testnet`.

## Money

| What | Real money? | Cost |
|---|---|---|
| Everything on-chain (xUSD-FAKE, test NEAR) | No, fake test tokens | 0 |
| Anthropic calls (B, A) | Yes, Alex's key | about $0.04 each; $0.03 spent so far; hard cap $1.50 total |
| NEAR AI credit (C) | Yes | about $5, Alex buys it, optional |
| Vercel hosting | No | free Hobby plan |

## Order

| # | Step | Who | Points |
|---|---|---|---|
| 1 | B: AI buyer re-run that must scan both agents | Cloud code, PC run | +0.5 |
| 2 | Hosted page on Vercel | Alex login, Cloud config | +0.75 |
| 3 | A: AI buyer buys on real testnet | Cloud code, PC run | +1.5 |
| 4 | C: NEAR AI signature verified on-chain (G6) | Alex credit, PC run | +1.5 |
| 5 | Video (docs/VIDEO.md), after 1-4 | Alex | +1.5 |
| 6 | Judge re-score | Cloud | |

## B: AI scans and refuses the cheat (about 30 min)

1. Cloud: change the default goal in `mcp/scripts/llm-buyer.ts` to make the
   model compare every listed agent and scan each before deciding (for
   example "Compare every listed agent. Scan each one before you decide. Buy
   the best safe one under 1,000 xUSD."). Keep the Skill text as is.
   Update the mocked-model tests (LB*) if they assert the goal. Push.
2. PC: `git pull`, `make mcp-build`, ONE run:
   `node mcp/dist/scripts/llm-buyer.js --provider anthropic --max-steps 12 --max-tokens 150000 --out out/llm-buyer.json`
   (`ANTHROPIC_WORKSPACE_ID` is already in `~/.agent-exit.env`). No retry.
3. Pass check: `summary.scanned_cheat` contains `blocked`, `cheat_bought`
   false, `translator_bought` true, `seller_locked_out` true. Commit
   `out/llm-buyer.json` (no `sk-ant-` inside).
4. Cloud: `make web-data`, update README "A real AI model bought an agent"
   line and docs/VIDEO.md 2:40 line; `make web-test`; push.

## Hosted page on Vercel (about 20 min, 5 of them Alex)

The page is static (`web/`) plus one function, `web/api/rpc.mjs` (the
read-only testnet relay for "Read live"). `web/vercel.json` sets the CSP.

1. Alex: sign in at vercel.com with GitHub (free Hobby plan).
2. Alex: Add New, Project, import `alexandergorokhov/near-transparent-agent-market`
   (private is fine). Settings: Root Directory `web`, Framework Preset
   "Other", Build Command empty, Output Directory `.`. Deploy.
   Send Claude the URL (`https://<name>.vercel.app`).
   Note: Vercel's own Git integration redeploys on each push to main; that
   is Vercel, not GitHub Actions.
3. Cloud: replace the `web-deploy` placeholder in the Makefile with a note
   on the Git integration (or `npx vercel@<pinned> deploy --prod --cwd web`
   for a manual deploy from the PC); put the URL in README line 3 and in
   docs/VIDEO.md; add a check that fetches the hosted page and asserts the
   CSP header and that `POST /api/rpc` refuses `broadcast_tx_commit` (400).
4. Cloud or PC: open `https://<url>/?network=testnet`, press Read live,
   expect `translator.agentexit3.testnet` owned by `summarizer.agentexit3.testnet`
   and code hash `7K1Q5DWY…`.

## A: AI buyer buys on real testnet (about 1 day)

Today the MCP server reads testnet but its write tools are not built
(`mcp/src/backend.ts`: "Writes on testnet are not built yet").

1. Cloud, code:
   - `TestnetBackend.buy`: sign `ft_transfer_call` on `xusd.<root>` from the
     buyer account with the listing version and key hash the scan saw (same
     args as `tools/testnet` scene 5). Use `near-api-js` pinned to an exact
     version, key from `AGENT_EXIT_PRIVATE_KEY` (server env only, never the
     model's). Keep the spend cap, `max_price`, `confirm` and the agent
     allow-list exactly as offline.
   - `verify_lockout` on testnet: `view_access_key_list` plus `get_status`.
   - Testnet setup step in `tools/testnet`: `scene-7` creates
     `buyer.<root>` (an agent account), mints it 1,500 xUSD-FAKE, and has
     the current owner list translator at 1,000; cheat stays listed.
   - `llm-buyer --network testnet` runs the same loop against the testnet
     MCP server and records NearBlocks links in `out/llm-buyer-testnet.json`.
   - Offline tests with a mocked RPC (no network in the cloud).
2. PC: `git pull`; `make scene-7` (testnet, fake money only); ONE capped run
   `node mcp/dist/scripts/llm-buyer.js --provider anthropic --network testnet ...`
   (about $0.05). Commit `out/llm-buyer-testnet.json` and `out/scenes/scene-7.json`.
3. Cloud: page step 6 shows the AI's buy with its NearBlocks link; README
   table row "An AI model buys translator on testnet"; tests; push.

## C: NEAR AI signature verified on-chain (G6, about 1 h)

1. Alex: add credit at https://cloud.near.ai (about $5, real money; the
   last attempt failed with 402 `no_limit_configured`). `NEARAI_API_KEY` is
   already in `~/.agent-exit.env`.
2. PC: `make llm-buyer-check` (one tiny call; must not be 402), then
   `make gate-g6`. Commit `out/gates/g6.json`.
3. Cloud: README "Not run on testnet" line loses G6; add the G6 row to the
   testnet table; page step 3 gets the on-chain Verified link; push.

## After all

Judge re-score (review 6), update STATUS.md, then Alex records the video.
