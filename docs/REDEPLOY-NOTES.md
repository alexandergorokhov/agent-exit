# Redeploy notes: the born-clean rule (NotBornClean)

The contract now refuses `list_for_sale` and the buy path (`NotBornClean`) for
any agent that the pinned factory did not create. This changes the contract
code, so the testnet world has to be redeployed and re-recorded. Until then
the recorded world (`agentexit5.testnet`, scenes, gates) is the OLD world.

What is committed in this state, and what is a placeholder:

| Item | State |
|---|---|
| Contract rule, tests, offline worlds, `make judge` 9/9 | final |
| `contracts/res/agent_exit.wasm` | built with `cargo near build non-reproducible-wasm` (no Docker here), pins `agentexit5.testnet`; sha256 `d1102d9c848614df8dde1a5e060e7b5340b23d147e3af831d4fe52c8d1f828d3`, code hash `F56WyouMoT7N6zQSEVHWtJz1zqJDutJVaRPjvDXy6VwY`. PLACEHOLDER: replace with the Docker `make build` result for the new world |
| `agent_code_hash` in `web/data/testnet.json`, `out/testnet/deploy.json`, `.mcp.json` | set to the placeholder hash above so K0 passes offline; the redeploy overwrites them |
| Testnet transactions for K8 (the cheat's refused `list_for_sale`) | not recorded yet (scene 4 records them) |

## TODO-REDEPLOY (one list)

Every place that names the world, the code hash or a transaction link. After
the redeploy and the re-recording, search `agentexit5`, `agentexit4`,
`5VqS52NR`, `F56WyouM`, `42d1c5d9`, `d1102d9c`, `70d06b2` and `nearblocks`
and replace each hit from the new `out/testnet/deploy.json` and `out/scenes/`.

1. `.mcp.json`: `AGENT_EXIT_CODE_HASH`, the agents list, the world root (use `node scripts/judge-mcp-config.mjs`; it reads `out/testnet/deploy.json`, and uses `deploy.factory` for the `translator.factory.<root>` names). `node scripts/judge-mcp-config.mjs --check` must pass.
2. `out/testnet/deploy.json`, `out/testnet/accounts.json`, `out/testnet/seed.json`, `out/scenes/*.json`, `out/gates/*.json`: written by the make targets below, never hand-edited.
3. `web/data/testnet.json`: `make web-testnet-data` (rewrites it from `out/`). Check `agent_code_hash` and the world name.
4. `web/data/diligence.json`: `node scripts/diligence-data.mjs` (it reads the new scene 4: the cheat's refused listing).
5. `contracts/res/agent_exit.wasm`, `contracts/res/SHA256SUMS`: from the Docker `make build` on a clean committed tree.
6. `tests/golden/attack.json` (the `code_hash` is read from SHA256SUMS): `make golden`, review the diff.
7. `README.md` (testnet table, world name, code hash `5VqS52NR…`, commit `70d06b2`, tx links, the K8 and K9 lines).
8. `llms.txt`, `AGENTS.md` (live world `agentexit5.testnet`, code hash, `K9 = gate G6` recorded on `agentexit4.testnet`).
9. `docs/PROOF.md` (rows with the code hash, `git checkout 70d06b2 && make build`, NearBlocks links, the scene 4 row and BC23).
10. `docs/TESTNET.md`, `docs/RUNBOOK-testnet.md` (the first-run table and current-world sentence), `docs/OPERATIONS.md`, `docs/BUSINESS.md`, `docs/PITCH.md`, `docs/ROADMAP.md`, `docs/LIMITS.md`, `docs/DETAILS.md`, `docs/COMMANDS.md`, `docs/SUBMISSION.md`, `docs/VIDEO.md` (world name, hash, links, wasm sha256 `42d1c5d9…`).
11. `docs/SCENARIOS.md`: row BC23 ("to record after the redeploy"): fill in the scene 4 transaction.
11b. `docs/RUNBOOK-testnet.md` lines about "cheat is listed" (section 7 and the scene 7 row) and `docs/TESTNET.md` scene 4 row: the cheat is no longer listed in the new world.
12. `web/proof.html`, `web/app.js`, `web/market.js`, `web/scenarios.js` (world name and links), `web/test/smoke.mjs` and `web/test/relay.test.mjs` (fixtures that name the world).
13. `mcp/test/*.test.ts` that name the world (`check_claims_testnet`, `network`, `judge_friction`, `llm_testnet`, `born_clean`), `mcp/src/tools.ts`/`backend.ts` (default world in messages), `mcp/scripts/k0.ts` (K0 account names).
14. `schemas/mcp/buy_agent.output.v1.json` (example world), `tools/verify/tests/meta.rs` (checks that README and records agree), `tools/testnet-plan/src/lib.rs` (default root in messages).
15. K9 (gate G6) on testnet: it is recorded on `agentexit4.testnet`; the re-run of `make gate-g6` on the new world replaces it, and the "previous world" sentences (README, AGENTS.md, llms.txt, PROOF.md, COMMANDS.md) go away.

## Redeploy steps (run on the PC, owner's keys)

World name used here: `agentexit6.testnet` (any new name works).

1. Create and fund a new root: `bash scripts/testnet/new-root.sh agentexit6.testnet 12`
   (each faucet round brings about 10 NEAR; see the funding table). It writes
   `AGENT_EXIT_ROOT` and `AGENT_EXIT_ROOT_KEY` to `~/.agent-exit.env`.
2. `git pull`, then `make setup && make verify && make judge` (must print
   `judge: 9/9 claims pass`).
3. `make build` (Docker, clean tree). It compiles in
   `xusd.agentexit6.testnet`, `agentexit6.testnet`, the MPC signer and
   `factory.agentexit6.testnet`, and rewrites `contracts/res` and
   `SHA256SUMS`. Commit them.
4. `make deploy-testnet DRY_RUN=1`, then `make deploy-testnet`: creates
   accounts, deploys xusd and the factory, stores the agent code in the
   factory, creates `translator.factory.agentexit6.testnet` through it, deploys
   `cheat` as a plain contract (the planted backdoor), sets runner keys, locks
   the factory, seeds the world.
5. `make gates DRY_RUN=1`, then `make gate-g1` (key handover on a factory-born
   agent). `make gate-g6` spends NEAR AI credit (one chat completion); `make
   gate-g2` needs a Base Sepolia key and is optional.
6. `make scenes` (3, 4, 5, 1, 2, 6). Scene 4 records the cheat's refused
   `list_for_sale` (`NotBornClean`) and the refunded buy-anyway: this is
   the testnet evidence for K8.
7. `make scene-7`, then `make llm-buyer-testnet` (ONE capped Anthropic call;
   it spends Anthropic credit).
8. Optional, in this order: `make scene-8`, `make scene-9`, `make scene-10`,
   `make scene-12`, `make scene-11`, `make scene-13 EXCHANGE_AGENTS=3`,
   `make agent-invests-testnet`. Each agent they create is born through the
   factory (10.2 NEAR each). Check scene 8 in the RUNBOOK before running it: its
   five translations are real NEAR AI jobs when `NEARAI_API_KEY` is set (spends
   NEAR AI credit) and unverified payments otherwise.
9. `make web-testnet-data`, `node scripts/judge-mcp-config.mjs`,
   `node scripts/diligence-data.mjs`, `make golden`, then work through the
   TODO-REDEPLOY list above.
10. `make judge`, `make judge-testnet` (read-only; K1-K9 with K8 from the new
    scene 4), `make ci`, `make screen`.

## Funding (NEAR)

| Step | NEAR |
|---|---|
| `deploy-testnet` (accounts 30.2, factory 11.5, translator through the factory 10.2, margin 1; the plan refuses below 41.9) | about 42 |
| `gate-g1` (fresh agent through the factory 10.2, gas) | about 10.5 |
| `scenes` 3 4 5 1 2 6, `scene-7` | under 1 (gas) |
| `scene-8` (translator3 10.2, dave 0.5) | about 10.8 |
| `scene-9` (born1) | about 10.3 |
| `scene-10` (sealed1) | about 10.3 |
| `scene-13` (3 agents) | about 31 |
| Everything | about 115-120 |

The minimum to record scenes 1-6 with the K8 evidence: about 55 NEAR.

## Costs other than NEAR

- `make gate-g6`: NEAR AI Cloud credit (one completion; cents). Without credit it fails with HTTP 402 and K9 stays on the old world.
- `make llm-buyer-testnet`: Anthropic credit (one capped call).
- Everything else is test NEAR and xUSD-FAKE.

## Known gaps in this state

- `tests/sandbox` (real local node) was updated to create every sold agent through the factory (b1, sealed, stress, plus a new BC-S2 plain-deploy test), builds with `cargo check --tests`, but could not be run here (no sandbox binary). Run `make test-sandbox` on the PC. `str_s5_near_stake_knife_edge` is `#[ignore]`d: it needs an agent funded just above its stake, which a factory-born agent never is (the factory attaches at least 10 NEAR).
- `nft_transfer` (deed move) is not gated by the rule: no money passes through the contract on that path (see `docs/NOT-CAUGHT.md` N4).
