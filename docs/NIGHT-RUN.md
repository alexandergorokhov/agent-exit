# Night run (PC, unattended)

For the Claude session that runs on Alex's PC (Remote Control) while he sleeps.
Testnet and fake xUSD only. Run the steps in order and stop at the first
"STOP". Do not retry a paid step. Never print keys, and never trigger GitHub
Actions. Commit and push the outputs to `main` after each step that passes.

## Before Alex goes to sleep (Alex, 5 min)

| # | Check |
|---|---|
| A1 | PC set to never sleep tonight; WSL open |
| A2 | NEAR AI: $1 credit added and a spend limit set on the API key (optional; without it, scene 8 runs unverified) |
| A3 | Faucet: `agentexit3.testnet` holds at least 10 NEAR (needed for scene 8) |
| A4 | Say "run" in the thread |

## Steps

| # | Command (in `~/src/near-transparent-agent-market`) | Real money | Pass | On fail |
|---|---|---|---|---|
| 0 | `git pull && make mcp-build` | 0 | builds | STOP |
| 1 | `make scene-7` | 0 | `out/scenes/scene-7.json` has `"pass": true` | STOP |
| 2 | `make llm-buyer-testnet` (ONE run, Haiku, 12 steps, 150k tokens) | about $0.05 Anthropic | `out/llm-buyer-testnet.json`: cheat not bought, translator bought, old owner locked out | commit the output anyway; skip to 3; no retry |
| 3 | `make llm-buyer-check` | under $0.01 NEAR AI | not 401/402 | note it; scene 8 runs unverified |
| 4 | `make build` (reproducible Docker build; rewrites `contracts/res`) then `make hash` | 0 | hashes match | STOP |
| 5 | Root balance at least 10 NEAR (`near state` or the RPC view) | 0 | yes | STOP: tell Alex "faucet needed" |
| 6 | `make scene-8 DRY_RUN=1`, then `make scene-8` | NEAR AI under $0.01 | `out/scenes/scene-8.json`: `check.exactly_20_percent` true | commit the output; STOP |
| 7 | If step 3 passed: `make gate-g6` | under $0.01 NEAR AI | `out/gates/g6.json` `"pass": true` | commit the output |
| 8 | `make web-testnet-data && make web-test` | 0 | passes | commit what passed |

At the end, post one summary to the thread: what passed and failed (a table),
the real money spent, and the commit hashes.

## Hard limits

- Anthropic: one run only (step 2); total Anthropic spend stays under $1.50.
- NEAR AI: the spend limit on the key caps it.
- No mainnet, no account creation outside scene 8's two accounts, no repo visibility change.
