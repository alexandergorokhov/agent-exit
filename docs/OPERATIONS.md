# Operating Agent Exit: paying an agent, the handover, the testnet day

Moved out of the README so its first screen stays short. All money is `xUSD-FAKE`; testnet only.

## How to pay an agent (customers)

1. **Once per customer account, register a payer row:** call
   `storage_deposit` on the agent with 0.024 NEAR attached (A49). Without it
   your payments still arrive and count in the totals, but they are pooled as
   `other`: you do not appear as a distinct payer in `get_earnings`.
   `storage_balance_of(you)` shows whether you are registered.
2. **Pay:** `xusd.ft_transfer_call(agent, amount, msg)` with `msg` empty or
   `{"kind":"pay","job_id":"..."}`; add a `nearai` proof to get the payment
   marked verified. Payments under 0.10 are refunded.
3. **Leave (optional):** `storage_unregister` removes your own row and
   refunds the deposit to you, but only if the row never had a payment.
   `storage_withdraw` has nothing to give back (nothing above the row price
   is ever held) and returns your balance. `reclaim_payer_row(account_id)`
   lets anyone remove a row that has had no payment 30 days after its
   deposit; the deposit goes back to whoever paid it (A50). `storage_unregister`
   and `storage_withdraw` need 1 yoctoNEAR attached.

On testnet the plan registers the six demo customers (`p1`..`p6`) this way
before they pay translator.

**A real translator to pay.** `make translator-serve` starts a worker on
`127.0.0.1:8787`. `POST /job` with `{"payer":"p1.testnet","amount":"1000000","text":"Hello"}`
returns the translation and a `pay_msg`: the exact `msg` for step 2, carrying
NEAR AI's response signature. The worker puts the agent, job, payer and amount
into the request NEAR AI signs, so the signature pays for that job only. Real
mode needs `AGENT_EXIT_NETWORK=testnet`, `NEARAI_API_KEY` and
`AGENT_EXIT_TRANSLATOR_ACCOUNT` (the `.testnet` translator agent). Without
them it serves a recorded fixture signed with local test keys, labelled as
such in every answer (not a NEAR AI signature). There is no on-chain
`accept_job`; the contract needs nothing new for this.

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
- The key swap and the payout are separate receipts: the key batch either
  runs or does not, and the seller is paid only if it succeeded; otherwise the
  buyer is refunded. The listing is locked during settlement.
- Moving the deed with plain `nft_transfer` runs the same key deletion and
  pauses the agent until the new owner sets a runner key.

## Testnet day (owner's machine)

Everything below runs from the repo root on a machine that can reach NEAR
testnet, Base Sepolia and NEAR AI Cloud. Secrets stay in `~/.agent-exit.env`
(template: `.env.example`) and `~/.agent-exit/keys/` (0600); nothing prints
them, and the runner refuses to write a file in `out/` that would contain
one. Anything but testnet is refused. The runner is `tools/testnet` (Rust,
near-api); the plan it runs is `tools/testnet-plan` (offline, tested in
`make verify`).

Funding: the root account (`AGENT_EXIT_ROOT` in the env file, e.g. `agentexit.testnet`; the current recorded world is `agentexit4.testnet`)
needs about 30 NEAR for the deploy (`near_needed` in the dry run is
authoritative). The two agent accounts, `translator` and `cheat`, get 9.5 NEAR
each: their code (~5.3), the growth reserve the contract demands (1.83) and the
1 NEAR reserve, so the scan sees them solvent. The token gets 2.5, the people
0.5 to 1 each, and each G1 run creates a fresh agent with 9 more. Run `make deploy-testnet DRY_RUN=1` to see the plan.

| Step | Command | Writes |
|---|---|---|
| 0 | `make deploy-testnet DRY_RUN=1` (and `gates`, `scenes`) | nothing: prints the plan |
| 1 | `make testnet-setup` (npm install for the G2 relay) | `scripts/testnet/node_modules` |
| 2 | `make deploy-testnet`: checks `contracts/res/SHA256SUMS` and deploys that reproducible wasm (built by `make build` with the root's token/admin/MPC compiled in; it refuses a wasm built for another root), creates the accounts, deploys, runner keys, locks translator, seeds the fixed world | `out/testnet/deploy.json` |
| 3 | `make gates`: G1 key handover on a fresh agent, G6 NEAR AI signature verified by the contract, G2 MPC signs `pay_base` 5 times (latency) and the relay sends it on Base Sepolia | `out/gates/g1.json`, `g6.json`, `g2-sign.json`, `g2.json` |
| 4 | `make scenes` (order 3, 4, 5, 1, 2, 6; waits out an open Base authorization before listing) | `out/scenes/scene-N.json` |
| 5 | `make scene-10`: the sealed prompt. Fresh agent `sealed1` (about 9.5 NEAR) sells at 100.00 with a committed config; the seller posts a fake bundle; the buyer's dispute decrypts it on-chain and gets the 100.00 back | `out/scenes/scene-10.json` (`pass`, `check.*`) |
| 6 | `make scene-11` (after scene 8): a business day on `translator3`: the owner sets the constraints once; customers pay, the runner key buys supplier jobs until the cap refuses, dave is paid in every payment | `out/scenes/scene-11.json` |
| later | `make scene-N RECORDED=1` replays a recorded scene offline (tx links, real errors) | |
