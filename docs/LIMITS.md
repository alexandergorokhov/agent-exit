# Honest limits

The README keeps a summary; this is the full list. What is not caught by design: [NOT-CAUGHT.md](NOT-CAUGHT.md).

- The seller can keep copies of off-chain code, prompts, model weights and
  outside API keys. Nothing on-chain can take those back; the earn-out prices
  that risk in. What a sale can guarantee, it does: with a sealed config
  (SC1-SC13) the buyer receives exactly the prompt and tool setup the listing
  committed to, encrypted to a one-time key, or the escrow comes back (the
  contract decrypts and hashes it itself in a dispute), and the bundle names
  the API keys to rotate (page tab "Buy the brain").
- Fake payers on aged or exchange-funded accounts look like real customers.
  The meter flags only what it can see (self, sub-accounts, round trips).
  After the sale the earn-out releases at most 50% of each payment, so
  wash-paying then costs the seller money; before the sale it costs almost
  nothing, so the buyer's due-diligence card and the MCP `get_earnings`
  (`revenue_quality`) weigh it down: revenue from payers first seen in the
  last 7 days counts half, and few payers or a short history are flagged (N3).
  A buyer who trusts none of it can buy "pay from revenue" (the whole price
  held, `min_escrow_pct: 100` in MCP): then a faked history earns the seller
  nothing up front, and the card shows the cost to fake (PR1-PR7).
- After the sale the buyer could still refuse customer payments by raising
  the price floor (`set_limits`). That costs the buyer the same revenue and
  is not prevented; the earn-out floor covers it. Round trips the buyer
  creates do not reduce the seller's earn-out (A31).
- The registry admin and payment token are build-time constants (`AGENT_EXIT_ADMIN`, `AGENT_EXIT_TOKEN`); the defaults are the offline fixture accounts, and the testnet deploy sets real ones.
- After 1,000 new payout destinations per owner (3,000 over all owners), the owner can only withdraw to themselves or to accounts already paid (`PaidToFull`), so the round-trip flag stays correct.
- A contract cannot list its own access keys. A hidden full-access key is
  caught by the buyer's scan before the sale, not by the contract; if a
  hidden key ever calls the contract, the contract freezes (A23). Agents
  created by the born-clean factory (`contracts/factory`) never had a key:
  one batch creates the account, deploys the published code and calls
  `new`, with no `AddKey`, so the scan reports `born_clean: true` and N4
  and N12 cannot happen to them (BC1-BC15; on testnet after the next
  deploy, scene 9). The full
  "not caught by design" list is [`docs/NOT-CAUGHT.md`](NOT-CAUGHT.md).
- The scan's published code hash is the reproducible build's
  (`contracts/res/SHA256SUMS`, built in the pinned cargo-near Docker image);
  the testnet agents run exactly that code.
- The offline tests sign NEAR Verified payments with labelled local test
  keys; on testnet, gate G6 and scene 8 used real NEAR AI Cloud signatures.
  The NEAR AI Cloud signer address is set by the registry admin (not by the
  seller, who could otherwise allowlist their own key) and is checked
  against the real service on the owner's machine.
- If a payout's own callback (`on_transfer_out`) fails after the token
  refused the transfer, the credit-back is lost from the books: the contract
  then holds more tokens than it accounts for (never fewer). Callbacks get
  gas for their own work plus the whole state, which makes this unlikely.
- Payer rows, day rows (at most 400) and used NEAR AI signature ids (at most
  512 shared, each kept 7 days) live under their own storage keys, never in
  the state blob, so loading the contract costs the same with 1 payer or
  1,000 (A47). A payer gets its own row only after calling `storage_deposit`
  and attaching 0.024 NEAR: the row, a deposit record and 8 signature slots
  of its own (A49, A54; NEP-145 style: excess is refunded, a second
  registration is refunded in full). Payer rows are not capped, because each
  is paid by its depositor and nothing iterates over them (A50); empty rows
  can be removed (`storage_unregister`, `reclaim_payer_row`) and deposits are
  held back from what the owner can withdraw. Payments from an account
  without a row still count in the totals but are pooled into `other`.
  Day rows and shared signature ids are paid from the agent's NEAR balance;
  every capped table's room is reserved
  (`get_status.near_storage.growth_reserve`), which the owner cannot
  withdraw, so outsiders can never make the agent insolvent.
- A failed supplier payment returns to the treasury but still counts toward
  that day's cap (conservative).
- The MCP server's offline mode replays recorded contract results; a buy or
  listing that was not recorded is refused (`OFFLINE_NO_RECORDING`), never
  made up. Testnet mode reads two RPC providers and refuses any mainnet URL
  or account; it ran on testnet on 2026-11-04 (`out/llm-buyer-testnet.json`).
- Fixed from security review 2: the deed can no longer be sent to the
  agent's own account (`DeedToAgent`), and `nft_approve` charges its
  storage and refunds extra deposit (L3). The testnet scan reads the code
  hash, method list and status from one provider only, at possibly
  different blocks, with `open_auth_until` fixed at 0 (the contract still refuses a buy while one is open) and no sub-account
  list (L6). A NEAR Verified badge shows that whoever made the NEAR AI
  request named that payer and amount; that can be the seller, so it does
  not prove a real customer (L7). The test token's faucet registers
  callers at the token's storage cost (L8).
- NEAR Intents is mainnet-only and is not used.
- Mocked-runtime tests do not execute promises: they check the actions the
  contract schedules and drive callbacks by hand. The sandbox tests cover the
  real `AccessKeyNotFound` errors and token refunds on a local node; the
  testnet run of 2026-11-04 covers the same on the public network (see
  "Live on NEAR testnet").
- The agent wasm is about 619 KiB (633,861 bytes) and not size-optimised yet.
- **Born clean is live on testnet since 2026-11-11** (scene 9 on `agentexit5.testnet`). The agent's `new` accepts only itself or its parent factory as caller. Agents deployed before that build cannot be born clean; a buyer's scan still reports how each agent was made.
