# Scenarios: near-hackathon (Agent Exit)

Source: `design/brief-final.md` and `design/agent-acquirer/AGENT-EXIT.md`. Needs the owner's approval once. Each row becomes an automated test in the build repo; the "Test" column is the file and test name the builder creates. Supersedes the Agent Acquirer scenarios.

**Layers:** `unit` = contract unit test (near-sdk `testing_env!`, offline). `prop` = proptest. `fuzz` = cargo-fuzz. `sbx` = near-sandbox integration test (local node, no internet after the binary is cached). `ts` = vitest. `rec` = recorded testnet receipts or signatures verified offline. `net` = live testnet, run on Alex's machine. `e2e` = Playwright on the page.
`make verify` runs unit + prop + ts + rec + the offline attack corpus and compares the JSON summary with `tests/golden/verify.json`. `make test-sandbox` runs sbx. `make attack` runs the A-series.

## Fixed world (the numbers every test and the demo use)

Token: `xusd`, our mock NEP-141 test dollar, 6 decimals (Circle NEAR-testnet USDC accepted too if obtainable; disclosed as mock in the README). Amounts below in dollars.

| Item | Value |
|---|---|
| Accounts | `alice` seller, `bob` buyer, `carol` second buyer, `translator` honest agent, `cheat` backdoored agent, `summarizer` agent buyer and supplier, payers `p1`-`p6`, `x.alice` seller sub-account, `mallory` outsider |
| translator history before listing | 6 payers × 5 payments × 12.00 = 360.00 counted; 10 of those (120.00) NEAR Verified; 2 self-payments by `alice` × 50.00 = 100.00 flagged `self`; 1 payment by `x.alice` 40.00 flagged `seller_subaccount`. Gross 500.00 |
| cheat history | 500.00 gross, all from `alice` and `*.alice`: counted 0.00, verified 0.00. Plus one extra full-access key |
| Listing | price 1,000.00; earn-out 30% = 300.00; release share 50% of each counted post-sale payment; floor 20% of escrow = 60.00; window 30 days (sandbox: time travel; testnet demo: `window_secs = 600`) |
| Policy (feature 12) | price floor 5.00 per job; daily cap 50.00; allowed payees [`summarizer`]; split owner 80% / treasury 20%; Base daily cap 5.00 USDC; auth validity ≤ 1 h |
| Storage guards | min counted payment 0.10; a payer row needs `storage_deposit` with 0.024 NEAR (A49, A54), uncapped (A50); payments alone are pooled as `other` |

## S: user and machine scenarios

| id | Given / When / Then | Layer | Test |
|---|---|---|---|
| S1 | **List an honest agent.** Given `translator` with a runner function-call key it added itself and its setup full-access key deleted, when `alice` calls `list_for_sale(1000, terms)`, then the listing is stored with `listing_version = 1`, the client scan returns `clean` and the listing card validates against `schemas/listing-card.v1.json` | unit, sbx, ts | `contracts/agent/src/tests.rs::s1_list_clean`, `tests/sandbox/s1.rs`, `sdk/test/scan.test.ts::s1` |
| S2 | **Meter counts by payer and day.** When the fixed history is paid in, then `get_earnings` returns gross 500.00, counted 360.00, verified 120.00, flagged 140.00 (self 100.00, seller_subaccount 40.00), 6 counted payers, per-day buckets | unit, sbx | `s2_meter_fixed_history` |
| S3 | **NEAR Verified payment.** A payment whose msg carries a recorded NEAR AI Cloud signature over `model:sha256(request):sha256(response)`, with the request naming `translator` and the job id, counts as verified; the same amount without a signature counts but is not verified | unit, rec | `s3_verified_badge` (uses `fixtures/recorded/nearai-sig-*.json`) |
| S4 | **"Both earn $500."** `get_earnings` for `translator` and `cheat` side by side: 500/360/120 vs 500/0/0; JSON matches golden | unit, ts | `s4_side_by_side`, `tests/golden/s4.json` |
| S5 | **Person buys in one step.** `bob` runs the scan, then `ft_transfer_call(1000, msg={buy, listing_version:1, known_keys_hash, buyer_runner_pk})`. Then: deed owner = `bob`; `alice`'s runner key gone, `bob`'s runner key present (key list before/after printed); `alice` +700.00; escrow 300.00; status `Sold`; `owner_epoch` 1 → 2; one `sale` event with all fields | unit, sbx, net | `s5_buy_one_step` |
| S6 | **Wallet follows the name.** Before and after the sale the derived Base Sepolia address for path `wallet-1` is the same; after the sale a `bob`-approved `pay_base(1.00 USDC)` produces an MPC signature and the relayer's transfer lands | unit (mock MPC), sbx (mock MPC), net, rec | `s6_wallet_follows`, `tests/recorded/s6_base_tx.json` |
| S7 | **Earn-out from real revenue.** After the sale, 4 counted payments of 25.00 release 4 × 12.50 = 50.00 to `alice`; at window end the floor tops up 10.00; `alice` earn-out total 60.00; `bob` gets 240.00 back | unit, sbx | `s7_earnout_partial` |
| S8 | **Silence pays the floor.** No post-sale payments: at window end `alice` 60.00, `bob` 240.00 | unit | `s8_earnout_silence` |
| S9 | **Full earn-out.** 600.00 counted post-sale: releases cap at 300.00; nothing back to `bob` | unit | `s9_earnout_full` |
| S10 | **Faking doesn't pay.** `alice` pays the agent 100.00 after the sale: flagged `self`, releases 0.00. Property: for any payer, release from a payment ≤ 50% of it, so wash-paying always loses money | unit, prop | `s10_wash_after_sale`, `prop_release_le_half` |
| S11 | **Business inside limits.** A job paid 25.00 splits 20.00 to the owner's claimable and 5.00 to treasury; the runner pays `summarizer` 8.00 for a sub-task: allowed | unit, sbx | `s11_split_and_procure` |
| S12 | **Over-limit payment fails live.** After 8.00 spent today, the runner pays `summarizer` 45.00: fails `PolicyViolation:DailyCap` (53.00 > 50.00); balances unchanged | unit, sbx, net | `s12_over_limit` |
| S13 | **Only the owner sets limits.** `bob` (owner) calls `set_limits` and it applies; `alice` gets `NotOwner`; the runner gets `NotOwner` | unit | `s13_constraints_owner_only` |
| S14 | **Agent buys agent via MCP.** An MCP client (the `summarizer` agent) calls `list_agents` → `get_earnings` → `scan_agent` → `buy_agent{max_price:1000, confirm:true}` → `verify_lockout`; every response validates against `schemas/mcp/*.v1.json`; `verify_lockout` returns `locked_out: true` for `alice` | ts (sandbox-backed), net | `mcp/test/s14_agent_buys_agent.test.ts` |
| S15 | **Resale.** `bob` lists `translator` again; history travels; `carol` buys; `bob` and `alice` both locked out; the first sale's earn-out keeps releasing from counted revenue | unit, sbx | `s15_resale` |
| S16 | **Offline rerun.** `make verify` in a container with networking off (`docker run --network none`) exits 0 in < 5 min and writes `out/verify.json` equal to `tests/golden/verify.json` | ci | `scripts/ci/verify_offline.sh` |
| S17 | **AI judge path.** On a fresh clone, every command in AGENTS.md runs, every path in llms.txt exists, every JSON output names its schema version | ts | `tests/meta/agents_md.test.ts` |
| S18 | **Judge on the hosted page.** A judge with no wallet gets an in-browser testnet key funded by the relayer, pays `translator` 1.00, gets a fresh demo agent with a planted backdoor (scan blocks the buy), buys a clean one, runs the break-in panel: every attempt fails | e2e (mocked RPC in CI), net | `web/test/s18.spec.ts` |
| S19 | **Listing card + dynamic deed.** `nft_token` metadata `extra` holds meter totals, last scan result and policy summary, updated on each payment and sale; the card at `/cards/translator.json` validates | unit, ts | `s19_dynamic_metadata` |
| S20 | **Recorded claims replay.** Every testnet claim in the README has a recorded receipt in `fixtures/recorded/` that `make verify` checks offline (hashes, signer, method, outcome) | rec | `tests/recorded/*.rs` |
| S21 | **`make demo` is real.** Offline, in one command: a quick `make verify` subset (the 60-scenario replay and the attack report, each compared with its golden; no cargo test run), then the scripted agent-buys-agent replay, then it prints how to open the page. It never claims a model decided anything | ci | `tools/verify/tests/meta.rs::s21_demo_target_runs_the_offline_story` |
| S22 | **README hero reads in 60 seconds.** One-sentence pitch, a 3-command "try it", one sale-flow diagram, a "proven on a real node" line (sandbox tests passed on a local NEAR node at 609b135 and 63d597c) and a "Testnet: coming" placeholder come before the claims table. The attack numbers are worded as a regression suite: "19 attack families, each blocked by the rule written for it; 0/50 false positives on clean agents", A9 (Base) excluded until Base is live, A5 marked as a placeholder hash | ci | `meta.rs::s22_docs_do_not_overclaim`, `meta.rs::s22_readme_hero_comes_first` |
| S23 | **Page copy matches the evidence.** Offline mode never says "two RPC providers"; the planted-key heartbeat is labelled FROZEN, not a green OK; a reused NEAR AI signature is labelled counted-not-verified; step 6 is a "scripted replay" with a slot for the real AI buyer transcript; counts (19 families, 26 MCP tests) are current | e2e, ci | `web/test/smoke.mjs` (`S23`), `meta.rs::s23_page_copy_is_honest` |
| S24 | **Page supply chain.** The page ships a strict CSP meta tag (`default-src 'none'`, scripts and styles `'self'`, no inline, no remote script); `@hot-labs/near-connect` is vendored under `web/vendor/` with its SHA-256 pinned and checked, not loaded from a CDN | e2e, ci | `web/test/smoke.mjs` (`S24`), `meta.rs::s24_page_has_csp_and_vendored_connector` |

## E: edge cases

| id | Case | Expected | Layer | Test |
|---|---|---|---|---|
| E1 | Buy with a stale `listing_version` | Refused `ListingChanged`; full amount returned through `ft_on_transfer`'s unused amount | unit, sbx | `e1` |
| E2 | Buy with a wrong `known_keys_hash` | Refused `KeysChanged`; full refund | unit | `e2` |
| E3 | Underpay 999.99 / overpay 1,000.50 | Underpay refused, full refund; overpay: 0.50 returned | unit | `e3` |
| E4 | Second buy while the first is `Settling` | Refused `SaleInProgress`; second buyer fully refunded | unit, sbx | `e4` |
| E5 | Key batch fails (e.g. the buyer's runner key is malformed or already present) | Callback restores owner, listing and status `Listed`; `bob` refunded 1,000.00; key list unchanged; no seller payout | sbx | `e5_rollback` |
| E6 | Seller payout transfer fails (`alice` not storage-registered on `xusd`) | 700.00 credited to `claimable[alice]`; `alice` withdraws later; solvency invariant holds | sbx | `e6_payout_fallback` |
| E7 | Buy attached with too little gas for the callback | Refused before any state change `NotEnoughGas` | unit | `e7` |
| E8 | `ft_on_transfer` called directly by a non-token account | Panics `UnknownToken`; nothing recorded | unit | `e8` |
| E9 | Payment in a token not on the list | Refunded in full | unit | `e9` |
| E10 | Malformed or huge `msg` | Full refund, no state change, no panic in the token callback path (fuzz target) | unit, fuzz | `e10`, `fuzz_buy_msg` |
| E11 | Same NEAR AI signature used twice | Second payment counted, not verified (`SignatureUsed`) | unit | `e11` |
| E12 | Signature for a request naming another agent | Counted, not verified (`WrongAgent`) | unit | `e12` |
| E13 | Forged signature, wrong signer, `v` in {0,1,27,28}, malformed length | Only the allowed signer verifies; others counted, not verified, no panic | unit, fuzz | `e13`, `fuzz_eip191` |
| E14 | Admin adds a new signer, removes the old | Already-verified payments stay verified; new payments with the removed signer are not verified | unit | `e14` |
| E15 | Dust payment 0.05 | Refunded; no payer entry created | unit | `e15` |
| E16 | A payer without a registered row | Pooled into `other`; storage per payment bounded | unit, prop | `e16_unregistered_payers_pool_into_other`, `prop_storage_bounded` |
| E17 | Owner withdraws more than balance minus storage reserve | Refused `StorageReserve`; withdrawing everything above the reserve always works | unit, prop | `e17`, `prop_owner_can_withdraw` |
| E18 | `alice` uses the whole daily cap just before the sale | `bob`'s epoch starts with the full 50.00 cap | unit | `e18_cap_resets_at_sale` |
| E19 | Spend across the UTC day boundary | Cap resets by block timestamp day | unit | `e19` |
| E20 | Price 999.999999 | 70/30 parts sum exactly to the price; rounding goes to escrow; no overflow (`overflow-checks = true`) | unit, prop | `prop_split_exact` |
| E21 | `finalize_earnout` called twice or by anyone | Second call is a no-op; result identical | unit | `e21` |
| E22 | MPC `sign` fails or times out | Callback releases the reserved cap, records no issued auth, emits `sign_failed`; retry uses a new nonce | unit (mock), sbx (mock) | `e22` |
| E23 | `pay_base` with validity > 1 h; `list_for_sale` while an auth is still valid | Refused `ValidityTooLong`; refused `OpenApproval` until `max_valid_before` passes | unit | `e23` |
| E24 | Owner moves the deed with plain `nft_transfer` | Runs the same handover: old runner keys deleted, agent paused until the new owner sets a runner key | sbx | `e24` |
| E25 | Deed approval (`nft_approve`) present | `list_for_sale` refused; approvals cleared on any transfer | unit | `e25` |
| E26 | `alice` cancels the listing during `Settling` | Refused `SaleInProgress` | unit | `e26` |
| E27 | Runner method during `Settling` | Refused `SaleInProgress` | unit | `e27` |
| E28 | Relayer: 11th request per minute from one IP; receiver not ours; daily budget spent | 429; 403; 503 with a "come back tomorrow" JSON | ts | `web/api/test/relay.test.ts` |
| E29 | MCP `buy_agent` without `confirm`, above `max_price`, or with any scan finding | Refused before any transaction, JSON error with the reason | ts | `mcp/test/e29.test.ts` |
| E30 | MCP server started without a key in env | Read tools work; write tools return `WRITE_DISABLED` | ts | `mcp/test/e30.test.ts` |
| E31 | Two RPC providers return different key lists | Scan result `INCONCLUSIVE`; buy path refuses (fail closed) | ts | `sdk/test/scan.test.ts::e31` |
| E32 | No network at all | `make verify` passes; `make scene-N RECORDED=1` replays recorded receipts with explorer links | ci | `verify_offline.sh`, `scripts/scenes/test_recorded.sh` |
| E33 | `set_limits` with split ≠ 100%, negative or absurd caps | Refused `BadConstraints` | unit | `e33` |
| E34 | Agent account has a sub-account | Scan warns `subaccount_does_not_move` (warning, not a block) | ts | `sdk/test/scan.test.ts::e34` |
| E35 | `make help` for a target that only prints `not built yet` | The help line carries `(not built yet)`; built targets never carry the marker | ci | `meta.rs::e35_help_marks_unbuilt_targets` |

## A: attacks (`make attack`)

`make attack` (offline) generates a seeded corpus: `N=200 SEED=42` mutated agents spread over A1-A20 plus 50 clean agents, runs the client scan and the contract checks, and prints one table plus `out/attack.json` (schema `attack-report.v1`). Expected fixed result: in-scope 200/200 blocked, false positives 0/50. `make attack FIXTURE=judge.json` runs a judge's own planted agent (format in `fixtures/judge.example.json`). `make attack-sandbox` runs the same families against near-sandbox; `make attack-testnet` (Alex's machine) runs A1, A5, A17-A21 live and records receipts.

### Before the sale: the sale must be blocked

| id | Attack | Caught by | Expected |
|---|---|---|---|
| A1 | Extra full-access key on the agent account | client scan | BLOCK `full_access_key_present` |
| A2 | Extra function-call key, receiver = agent, owner-like methods | client scan vs `known_keys()` | BLOCK `unknown_key` |
| A3 | Extra function-call key, receiver = a foreign contract (e.g. `xusd`) | client scan | BLOCK `unknown_key` |
| A4 | Runner key added with the setup key, not through the contract | client scan vs `known_keys()` | BLOCK `unknown_key` |
| A5 | Code hash ≠ the published global-contract hash (patched wasm) | client scan | BLOCK `code_hash_mismatch` |
| A6 | Same interface plus an upgrade or `add_key` method | client scan (hash) | BLOCK `code_hash_mismatch` |
| A7 | Seller changes price or earn-out terms after the buyer scanned | contract | buy refused `ListingChanged`, refund |
| A8 | Open approval on the deed | contract | list/buy refused `OpenApproval` |
| A9 | Pre-signed Base authorization still valid after the sale | contract | list/buy refused `OpenApproval` |
| A10 | Wash earnings: self-payments | contract meter | flagged `self`, counted excludes |
| A11 | Wash earnings through seller sub-accounts | contract meter | flagged `seller_subaccount` |
| A12 | Round trip: the agent pays P, P pays the agent back | contract meter | flagged `round_trip` |
| A13 | Replayed or forged NEAR AI signature | contract | not verified |
| A14 | Fake token calls `ft_on_transfer` | contract | panic `UnknownToken` |
| A15 | Seller burns the daily cap before the sale | contract | cap resets at sale |
| A16 | Seller moves the deed to an accomplice outside `buy()` to keep the runner key | contract | runner keys deleted on any transfer |

### After the sale: every way back in must fail (the break-in scene)

| id | Attempt by the old seller | Expected real error |
|---|---|---|
| A17 | Sign a runner call with the old runner key | `InvalidAccessKeyError: AccessKeyNotFound` |
| A18 | `set_limits` from `alice` | `NotOwner` |
| A19 | `withdraw` / treasury payout from `alice` | `NotOwner` |
| A20 | Ask the Base wallet to sign (`pay_base`) | `NotOwner` |
| A21 | Use the setup full-access key deleted at listing | `InvalidAccessKeyError: AccessKeyNotFound` |
| A22 | `nft_transfer` the deed back | `NotOwner` (no approval left) |
| A23 | A hidden key that slipped past a skipped scan calls the contract as the agent itself | contract refuses any self-call whose signer key it didn't add: agent frozen, `backdoor_proven` event, remaining earn-out returned to the buyer |
| A24 | Seller drains the treasury while the agent is listed | `pay` refuses while listed ("Listed: spending paused"); `buy()` refuses if the treasury is below the listing snapshot |

### Review 3 and later (A43-A49)

| id | Attack or finding | Expected | Test |
|---|---|---|---|
| A43 | A large agent account (big state) buys or pays | Callback gas counts the state blob, not the account's code or per-entry storage, so it can still pay and sell | `review3_regressions.rs` |
| A44 | A key-batch callback lands after `recover_keys` / `recover_sale` | Stale (batch nonce): changes no key set; the late `on_handover` refunds | `review3_regressions.rs` (L1) |
| A45 | Payout destinations of an era a finalized sale closed | Pruned, so the table does not last the agent's whole life | `review3_regressions.rs` (L7) |
| A46 | A NEAR AI proof older than the window, ahead of the block, or without `signed_at` | Not verified (the payment still counts) | `review3_regressions.rs` (M2) |
| A47 | **Outsiders brick an agent by paying it.** Found by a stress run on a local NEAR node at 12d1ab7: about 585 distinct payers made every call trap while loading state. Offline tests did not show it. | Payer and day rows live under their own storage keys; the state blob is the same size at 1,200 64-character payers; views cost the same at 10 and 1,000 payers; after 1,000 payers of 64 characters the agent can still be bought and every view works | offline blob and gas tests; sandbox `a47_thousand_long_payers_then_buy_and_views` |
| A48 | A deed move (`nft_transfer`) | The old owner's unwithdrawn revenue stays theirs | `a48_deed_move_keeps_old_owners_revenue_theirs` |
| A49 | **Payer rows cost NEAR.** Payments from an account with no row are pooled into `other` and add no row. `storage_deposit` with 0.024 NEAR (it was 0.002 before A54) registers a row. | Less than 0.002 NEAR: `StorageDepositTooSmall` (deposit refunded); excess refunded; a second registration refunded in full; a full table: `PayersFull`; a 64-character account fits the deposit. How to pay: `storage_deposit` first, then `ft_transfer_call` | `a49_payer_rows_cost_near` in `review3_regressions.rs` |
| A50 | **No cap on payer rows; each is paid by its depositor** (review 4 M1): 2 NEAR could fill the old 1,000-row table | Nothing iterates over the rows, so no cap. `storage_unregister` removes the caller's own row if it never had a payment; `reclaim_payer_row` lets anyone remove a payment-less row 30 days after its deposit (refund to the depositor); deposits are held back from what the owner can withdraw | `review4_regressions.rs` (`a50_*`), `agent-core/tests/meter.rs` |
| A51 | **The Base wallet address is checked by the scan** (review 4 M2) | The scan derives (agent, `wallet-1`) from the MPC root key and blocks `base_address_mismatch`, or `base_wallet_unchecked` when it cannot check | attack family A51 in `make attack`; `tools/scan/tests/scan.rs::a51_base_address_is_derived_from_the_mpc_key` |
| A52 | `set_limits` without Base fields (review 4 L1) | Keeps the Base limits; an explicit zero cap and empty payees clears them | `a52_set_limits_without_base_keeps_base_limits` |
| A53 | Owner epochs grow the state blob (review 4 L2) | Counters and a pruned-prefix watermark replace per-epoch maps; nothing is pruned while a sale is open | `a53_*` |
| A54 | Filling the shared signature window denies a payer's verified badge (review 4 L3) | A registered payer has 8 signature slots of its own, prepaid by its row deposit (0.024 NEAR in all) | `a54_*` |

### Not caught by design (printed as rows N1-N6 in every `make attack` run)

| id | Not caught | Why |
|---|---|---|
| N1 | Copies of off-chain code, prompts, model weights | Not on-chain; the earn-out prices it in |
| N2 | Outside API keys and accounts (LLM keys, market.near.ai tokens, customers' contacts) | Not on-chain |
| N3 | Sybil payers on aged or exchange-funded accounts | Can't be told apart on-chain; earn-out share < 100% makes it cost money |
| N4 | A hidden full-access key used directly (not through the contract) by a buyer who skipped the scan | The contract can't list its own keys; the SDK and MCP never skip the scan |
| N5 | Customers moving off-platform after the sale | Business risk, not a key |
| N6 | A compromised MPC network, NEAR AI signer or both RPC providers | Trust assumptions, listed in the threat model |
| N7 | That `make attack` is independent evidence of detection | The corpus (`tools/scan/src/corpus.rs`) is written by the same author as the rules: each family's expected finding is the code of the rule written for it. It is a regression suite. A9 (open Base authorization) is excluded from the headline count until Base is live (the testnet scan fixes `open_auth_until` at 0); A5 compares against a placeholder hash until the reproducible build exists |

## Added during the build (MCP, llm-buyer, testnet relay)

| id | Scenario | Layer | Test |
|---|---|---|---|
| LB1 | **A real model buys through MCP (llm-buyer).** A model reads the Skill, scans, refuses `cheat`, buys `translator` and verifies the lockout (tested with a mocked model client against the real MCP server, offline) | ts | `mcp/test/llm_buyer.test.ts` |
| LB2 | The server, not the model, enforces `max_price`, `confirm` and the session spend cap | ts | `llm_buyer.test.ts` |
| LB3 | Unknown tools and bad arguments from the model are dropped | ts | `llm_buyer.test.ts` |
| LB4 | The API key appears only in the auth header: never in tool results, transcript or output | ts | `llm_buyer.test.ts` |
| LB5 | No API key: exit 2 | ts | `llm_buyer.test.ts` |
| LB6 | The env file is read for model variables only; base URL guards | ts | `llm_buyer.test.ts` |
| LB7 | An endless or erroring model fails the run (step limit, model errors) | ts | `llm_buyer.test.ts` |
| LB8 | The model is the cheapest chat model that supports tools, else the default (`deepseek-ai/DeepSeek-V4.1-Flash-1017`); the choice is always logged | ts | `mcp/test/llm_buyer_nearai.test.ts` |
| LB9 | Token usage is recorded per reply and in total | ts | `llm_buyer_nearai.test.ts` |
| LB10 | HTTP 401 and 402 end the run with a clear message and are not retried | ts | `llm_buyer_nearai.test.ts` |
| LB11 | A key pasted several times in the env file is detected (warning) without printing it | ts | `llm_buyer_nearai.test.ts` |
| LB12 | `make llm-buyer-check`: one tiny authenticated chat call; 401/402 and no key get clear errors | ts | `llm_buyer_nearai.test.ts` |
| S-PG | `get_sales(from_index, limit)` is paginated, so the sale history never has to be read whole | unit | `contracts/agent` |
| L6 | **The page is served with security headers.** `web/serve.mjs`, `web/_headers` and `web/vercel.json` send the CSP (the page's own plus `frame-ancestors 'none'`), `X-Content-Type-Options: nosniff` and `Referrer-Policy: no-referrer`, also on 404s. The vendored connector's remote wallet manifests stay blocked until their hosts are reviewed (`web/vendor/README.md`) | node | `web/test/smoke.mjs` |
| M4-L4a | On testnet, `buy_agent` only buys agents listed in `AGENT_EXIT_AGENTS`, checked before any read | ts | `mcp/test/review4.test.ts` |
| M4-L4b | The per-day spend cap persists per account in `out/mcp-spend.json` (testnet); a restart does not reset it; offline stays in memory | ts | `review4.test.ts` |
| M4-L5 | The model env file is refused unless it is mode 600 | ts | `review4.test.ts` |
| T-S6 | G2 relay: the value, from, to and nonce inside the calldata must equal the authorization | node | `scripts/testnet/test/g2.test.mjs` |
| T-S7 | G2 relay: `AGENT_EXIT_NETWORK` must be set, and be testnet | node | `g2.test.mjs` |
| T-S8 | G2 relay: a chainsig.js address that differs, or cannot be derived, stops the relay | node | `g2.test.mjs` |
| T-S9 | Secret files must be mode 600; the env file is refused when group- or world-readable | node | `g2.test.mjs` |
| S25 | **README first screen.** A screenshot (`docs/img/demo.png`, `web/test/screenshot.mjs`), three headline numbers, the 3-command try-it and a "Why only on NEAR" box come first; the proof table lives in `docs/PROOF.md` | ci | `meta.rs::s25_readme_first_screen` |
| S26 | **No overclaims in wording.** The page says "Agent Exit · sell an AI agent safely", not "marketplace"; the handover is "one transaction" and payouts are claimable afterwards, never "all or nothing" for the whole sale | ci | `meta.rs::s25_readme_first_screen`, `smoke.mjs` |
| S27 | **No wallet login button** until testnet mode is live (`TESTNET_LIVE` in `web/app.js`); the "Why only on NEAR" box is on the page | node | `web/test/smoke.mjs` |
| S28 | **`make demo` prints a short summary**; the JSON goes to `out/` | ci | `meta.rs::s25_readme_first_screen` |
| TR1 | `translator-serve` offline: a recorded fixture, labelled as such, in the shape the contract checks | ts | `mcp/test/translator.test.ts` |
| TR2 | Real mode: the signed request names agent, job, payer and amount; the proof matches the signed bytes; a signature that does not sign `model:req:resp` is never handed out as a proof | ts | `translator.test.ts` |
| TR3 | Job input is validated (limits) before anything is sent | ts | `translator.test.ts` |
| TR4 | The HTTP server listens on loopback only; `POST /job`, health, size and job limits | ts | `translator.test.ts` |
| TR5 | Start-up gating: offline by default; real mode needs testnet, a key and the agent account, else exit 2 | ts | `translator.test.ts` |
| TR6 | NEAR AI 401 and 402 are clear, not retried, and the key is never repeated | ts | `translator.test.ts` |
| TR7 | Hostile text is data only: it sits in the user message, the system prompt is fixed | ts | `translator.test.ts` |
| M-L2 | `sent: true` after a buy that was sent but whose follow-up read failed; the spend is kept | ts | `mcp/test/review3_lows.test.ts` |
| M-L3 | The scan child process gets PATH only | ts | `review3_lows.test.ts` |
| M-L4 | Testnet `verify_lockout` can be inconclusive | ts | `review3_lows.test.ts` |
| M-L5a-e | RPC timeout and no redirects; wasm parser bounds; `.testnet`-only accounts; sanitised error text | ts | `review3_lows.test.ts` |
| M-AL | Allowance pre-check: `ALLOWANCE_LOW` before a buy is sent | ts | `mcp/test` |
| T-S1 | G2 relay dry run prints the plan, reads no secret, writes nothing | node | `scripts/testnet/test/g2.test.mjs` |
| T-S2 | G2 relay: testnet only | node | `g2.test.mjs` |
| T-S3 | G2 relay: Base RPC allowlist; no URL leak | node | `g2.test.mjs` |
| T-S4 | G2 relay: the relayer key is checked without being echoed | node | `g2.test.mjs` |
| T-S5 | G2 relay: only `transferWithAuthorization` calldata is relayed | node | `g2.test.mjs` |
