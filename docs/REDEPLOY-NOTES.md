# Redeploy notes: the born-clean rule (NotBornClean)

Done on 2026-11-13. The contract refuses `list_for_sale` and the buy path
(`NotBornClean`) for any agent the pinned factory did not create. The testnet
world `agentexit6.testnet` runs exactly the committed reproducible build
(`contracts/res/agent_exit.wasm`, sha256 `259d414a75fb83010140fa27cdc29e20cce79b4674600860ff2e5813a25e827e`,
code hash `3XqAPt3krfpJG9UWFHnZVyDYSws4ZnN4H3B27hCXPuuw`, built from commit `1dc6ca1`).
Scene 4 recorded the cheat's refused `list_for_sale`
([BQrAUGB6…](https://testnet.nearblocks.io/txns/BQrAUGB6yMia6Ep4ZdTSWegoVgZqfP7LJPJV8Dnh99Pu))
and the refunded buy ([2Rf95RJf…](https://testnet.nearblocks.io/txns/2Rf95RJft2qHJbtJfXKYGihbnLjtFbpLinuL8wkgQP6H)).
`make judge-testnet` passes K1-K9 live. Two records were not repeated on the new
world: `make agent-invests-testnet` (a per-account daily spend cap refused it) and
the G2 signing half; both stay recorded on `agentexit5.testnet` and are labelled so.

Below: the steps to redo this on a fresh world (run on the PC, owner's keys).

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
   `node scripts/diligence-data.mjs`, `make golden`, then search the docs for
   the old world name, code hash and transaction links and replace each hit from
   `out/testnet/deploy.json` and `out/scenes/`.
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

- `make gate-g6`: NEAR AI Cloud credit (one completion; cents). Without credit it fails with HTTP 402.
- `make llm-buyer-testnet`: Anthropic credit (one capped call).
- Everything else is test NEAR and xUSD-FAKE.

## Known gaps

- `tests/sandbox` (real local node) was updated to create every sold agent through the factory (b1, sealed, stress, plus a new BC-S2 plain-deploy test), builds with `cargo check --tests`, but could not be run here (no sandbox binary). Run `make test-sandbox` on the PC. `str_s5_near_stake_knife_edge` is `#[ignore]`d: it needs an agent funded just above its stake, which a factory-born agent never is (the factory attaches at least 10 NEAR).
- `nft_transfer` (deed move) is not gated by the rule: no money passes through the contract on that path (see `docs/NOT-CAUGHT.md` N4).

## Page and video wording that must change after the redeploy (K8)

Until the redeploy, the pages and the video describe the OLD recorded world for the cheat (listed at 500.00, scan says `full_access_key_present`, buy never sent) and add the K8 rule as the contract's rule (offline, `make judge` K8). After the redeploy and re-recording:

1. `web/market.js`: the QuickQuote card line "New rule: the contract now refuses to list agents that ever had a key (K8, offline; on testnet after the redeploy)" becomes plain fact; the cheat card already switches to "listing refused (NotBornClean)" when scene 4 records a `NotBornClean` tx.
2. `web/app.js`: the `dd-k8` note in the QuickQuote case file and the `cheat` line in `SHORT` ("The buyer's scan finds it and blocks the sale") become "the contract refuses to list it".
3. `web/proof.html` and `README.md`: drop the "predates this rule" sentence in Honest limits and the "(K8, tested offline; on testnet after the redeploy)" text on `web/index.html`.
4. `scripts/video/script.mjs` and `record.mjs`: the `cheat` scene says the scan caught the key and that the contract now refuses to list a keyed agent. After the redeploy, show the real refused `list_for_sale` tx (scene 4) and drop "offline". Re-record, mix, encode; update the length in `docs/VIDEO.md`, `web/proof.html` and `README.md`.
5. The page claims "the live world predates K8" must go once `web/data/testnet.json` has the NotBornClean tx.
