# Agent deed: a standard for selling and investing in agent accounts

Status: draft, written from the code that runs on NEAR testnet today (`contracts/agent/src/lib.rs`, rules in `contracts/agent-core`, factory in `contracts/factory`). Nothing below is a proposal for code that does not exist yet; open points are listed at the end.

## Why a standard

An AI agent that earns money is a small business. To invest in one, or to buy it, a stranger needs three answers they can read from the chain, without trusting the seller:

1. **Who owns it?** One token says so.
2. **Is the revenue real?** Every payment is counted by who paid it.
3. **Can the seller come back after the sale?** The sale removes the seller's keys in the same transaction that pays them.

This document describes the interface an agent account exposes so that any wallet, marketplace or AI buyer can answer those three questions the same way. Agent Exit is one marketplace on top of it; the interface is the part other agents can adopt.

## Terms

- **Agent account**: the NEAR account the agent runs as. The contract below is deployed on it, so the account and the business are the same thing.
- **Owner**: whoever holds the deed. Acts from their own account.
- **Runner key**: the one function-call key the agent software signs with. Only the contract adds it.
- **Payment token**: the one NEP-141 token the build accepts (pinned at build time as `PAYMENT_TOKEN`).

## 1. The deed (NEP-171, issued by the agent's own account)

The agent account is its own NFT contract with exactly one token, `token_id = "deed"`. It is minted in `new(owner_id, base_address)` (event `nft_mint`, memo `agent_exit:deed`). Whoever holds it is the owner.

- Views: `nft_token`, `nft_tokens`, `nft_total_supply` (always 1), `nft_supply_for_owner`, `nft_tokens_for_owner`, `nft_metadata`, `nft_is_approved`.
- Transfers: `nft_transfer`, `nft_transfer_call`, `nft_resolve_transfer`, `nft_approve`, `nft_revoke`, `nft_revoke_all` (1 yoctoNEAR where NEP-171 asks for it).
- Rule: moving the deed outside a sale deletes every runner key and pauses the agent until the new owner calls `set_runner_key` (event `deed_moved`). A deed never changes hands with the old operator still inside.
- `new` may only be called by the account itself or its direct parent (`InitNotAllowed`), so nobody else can initialise someone's agent.

## 2. The payer-counted revenue meter

Every payment arrives as `ft_transfer_call` on the payment token, with `msg` empty or `{"kind":"pay","job_id":...,"nearai":...}`. A call from any other token panics `UnknownToken`. Each payment is classified by who paid it (`flags_for`):

| Flag | Set when the payer is |
|---|---|
| `self` | the agent, its owner, any past owner, or an ancestor of the agent account (`alice.testnet` for `bot.alice.testnet`) |
| `seller_subaccount` | a sub-account of any of those |
| `round_trip` | an account the agent sent tokens to before |

Rules:

- **Counted** = no flag. Flagged payments are still accepted and recorded, but never count as revenue.
- **Verified** = counted, and the payment carries a NEAR AI Cloud signature the contract checks itself (`verified.rs`: EIP-191 hash, `env::ecrecover`, signer in the admin-set list from `get_signers`, request metadata naming this agent, job, payer and amount, each signature used once, at most 7 days old). A failed proof leaves the payment counted, not verified.
- Payments below 0.10 (`Dust`) or below the owner's price floor are refused; a frozen agent refuses all (`Frozen`). A refusal returns the whole amount through the token's refund.
- A payer becomes a distinct row by paying `storage_deposit` (NEP-145 style; `storage_balance_of`, `storage_unregister`, `storage_withdraw`, `storage_balance_bounds`, `reclaim_payer_row`). Payers without a row are pooled.

Event `payment` (`payer`, `amount`, `job_id`, `counted`, `verified`, `verify`, `flags`, `day`), then `split` (owner, treasury, investors, earn-out release). Views: `get_earnings` (`earnings.v1`: `gross`, `counted`, `verified`, `flagged`, flagged by kind, `counted_payers`, `fresh_payers`, the newest 31 day rows), `get_earnings_days(from_index, limit)`, `get_payer(account_id)`.

## 3. The one-transaction handover

Listing (owner, 1 yoctoNEAR): `list_for_sale(price, code_hash, terms)` returns `listing.v1` with `listing_version`, `known_keys` (the keys the contract added) and `known_keys_hash`. `cancel_listing` withdraws it. View: `get_listing`.

Buying is one `ft_transfer_call` of the price with

```json
{"kind":"buy","listing_version":7,"known_keys_hash":"<hex>","buyer_runner_pk":"ed25519:...","delivery_pk":null}
```

The buy is refused if the version or the key hash changed since the buyer scanned. Otherwise, in one receipt on the agent's own account:

1. delete every runner key the seller had (`delete_key`);
2. add the buyer's runner key, a function-call key scoped to this account and `heartbeat, accept_job, pay, pay_base`, no deposit allowed, a 5 NEAR fee allowance;
3. in `on_handover`: the deed moves to the buyer (`nft_transfer` event, memo `agent_exit:buy`), the seller is paid now, the rest goes into escrow; the event `sale` lists price, `paid_now`, `protocol_fee`, `escrow`, `window_end`, `known_keys`.

If the key batch fails, the buyer gets the whole payment back (`sale_rolled_back`). If the callback itself never lands, anyone may call `recover_sale` an hour later. Events in order: `settling`, `nft_transfer`, `sale`.

**Escrow (earn-out).** `terms` = `earnout_bps`, `share_bps`, `floor_bps`, `window_secs`. The published build enforces at least 30% held, a floor of at most half the escrow, a window of at least 30 days (`terms_min` in `get_status`). During the window, `share_bps` of each counted payment from a payer not linked to the seller releases escrow to the seller (`earnout_release`). After it, anyone calls `finalize_earnout(sale_id)`: the seller gets at least the floor, the rest goes back to the buyer. `earnout_bps = 10000` means the seller is paid only from revenue.

**Backdoor proof.** Every runner method checks the signing key. A self-call signed by a key the contract never added proves a hidden key: the agent freezes and open escrow goes back to buyers (event `backdoor_proven`).

**Lockout after the sale.** The old seller's runner key is gone (`AccessKeyNotFound`); owner methods (`withdraw`, `set_limits`, `list_for_sale`, `nft_transfer`, `set_runner_key`) answer `NotOwner`; runner methods (`pay`, `pay_base`) answer `NotRunner`.

Optional: a sealed agent config (system prompt, tools) sold with the agent: `commit_config`, `request_preview`, `answer_preview`, `get_preview`, `deliver_config`, `dispute_config`, `claim_undelivered`, `release_upfront`, `get_sealed_config`.

## 4. Shares of future earnings

The owner sells a slice of every future payment without selling the agent.

- `offer_share(bps, price, end)` (owner, 1 yoctoNEAR); `cancel_offer(offer_id)` for an unsold slice.
- Buy with `ft_transfer_call`, `msg = {"kind":"buy_share","offer_id":..,"bps":..,"price":..,"end":..,"part":..}` (the terms the investor saw; a change refuses the buy).
- Inside every payment, each holder's part of the gross amount is credited to the holder's claimable balance (`share_paid`); the owner never holds it. Holders call `claim`.
- Half of what an investor pays waits in escrow and reaches the owner only as the holder is paid from counted payments. At `end`, anyone calls `close_share(offer_id)` and what is still held goes back to the investor.
- Bounds: open slices at most 49% in total, at most 2 open slices and 16 holders, each slice 1% or more, 1 day to 2 years.
- Views: `get_shares`, `get_holdings(account_id)`, `get_claimable(account_id)`. A buyer of the whole agent inherits open slices; `list_for_sale` shows them.

Events: `share_offered`, `share_bought`, `share_paid`, `share_cancelled`, `share_closed`.

## 5. Born clean (factory, no key ever)

A factory (`contracts/factory`) creates `name.<factory>` in one batch: create the account, fund it, deploy the stored code, call `new(owner)`. No access key is ever added, so the account has never had one.

- `create_agent(name, owner_id, base_address)` (at least 10 NEAR attached, refunded if creation fails); event `agent_born` (standard `agent_exit_factory`).
- `store_code` accepts only bytes whose sha256 is the code hash fixed at `new(code_hash)`.
- Views: `get_agent(account_id)` (owner at birth, code hash, block), `list_agents`, `get_code_hash`, `total_agents`, `has_code`.
- A buyer checks `get_agent` instead of trusting a key scan at one moment in time: a factory-born agent never had a full-access key and had no state before the published code ran.

## Events and status

All agent events are NEP-297 logs: `EVENT_JSON:{"standard":"agent_exit","version":"1.0.0","event":...,"data":[...]}`. The deed also emits standard `nep171` events. `get_status` (`agent-status.v1`) returns owner, status, `known_keys`, `known_keys_hash`, `frozen`, `paused`, `terms_min`, escrow and share totals, and NEAR storage solvency in one call.

## How an existing agent adopts it

An agent that already runs on its own NEAR account:

1. Deploy the published contract to the account and call `new(owner_id, base_address)` in the same transaction (`base_address` may be omitted).
2. As the owner, call `set_runner_key(public_key)` with the key the agent software will use, and switch the software to it.
3. Delete every other access key on the account, full-access keys included. After this, the keys on the account equal `known_keys`.
4. Ask customers to pay with `ft_transfer_call` on the payment token (optionally `storage_deposit` first, so each customer is its own row). To earn the verified mark, attach the NEAR AI Cloud signature in `msg`.
5. Optional: `set_limits` for the agent's own spending, `offer_share` to raise money, `list_for_sale` to sell.

A new agent skips steps 1 to 3 by being created through the factory.

What adopting does not give an existing agent: its history before step 1 is not counted, and a buyer cannot prove from the chain that nothing was written before the contract ran. Only a factory-born agent proves that.

## Open questions

- **One payment token per build.** The token, the registry admin and the fee account are compiled in, so the code hash pins them. Should the standard allow a list of tokens, and how does the code hash stay meaningful then?
- **Who maintains the NEAR AI signer list?** Today one registry admin can add or remove signing addresses (`add_signer`, `remove_signer`) and nothing else. A shared, on-chain registry would serve every agent.
- **Flags see account names, not people.** A seller paying through an unrelated account is counted until the earn-out window. The escrow is the answer today; a payer reputation shared across agents is not specified.
- **Off-chain state.** A seller can keep a copy of the prompt. The sealed config makes swapping it disputable, not copying it impossible.
- **Chain Signatures wallet.** The Base address is set at `new` and stays with the account across a sale; the NEAR-side lockout of `pay_base` is live on testnet, a real Base Sepolia payment is not recorded yet.
- **Upgrades.** The contract has no upgrade method and a locked account has no key to deploy with. A fix means a new agent and a migration path, which is not specified.
- **NEP number.** This is written in NEP style but has not been submitted; field names may change in review.
