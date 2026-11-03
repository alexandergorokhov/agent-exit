# Plan: invest in agents (shares) + a translator that really works

Saved 2026-11-03. Alex approved options 1 and 2 after cold judge 7 (38/60, Impact 4/10).
Goal: raise Impact by turning Agent Exit from "buy a whole agent" into "invest in agents",
where every payout comes from earnings you can check on chain.

New pitch: **"Invest in AI agents. Buy a share of an agent's future earnings, or the whole agent. Each payment is counted and split on chain."**

The core stays the same: the earnings meter, the scan and the one-transaction handover. Shares are a second way to put money in, and they reuse the same meter.

## 1. Shares of future earnings (option 2)

### How it works (plain words)

1. The owner offers a slice, for example "20% of earnings for 12 months, 500 xUSD". They choose the percentage, the price and the end date.
2. An investor buys it with one `ft_transfer_call`. The price goes to the owner. The investor does not need to buy the whole slice.
3. Each customer payment into the agent splits automatically: 20% to the investors, pro rata, credited as claimable, and the rest to the owner. The split runs inside `pay()`, so the owner cannot skip it.
4. The investor claims their share at any time.
5. When the agent is sold, its shares come with it. The buyer's scan shows "20% of earnings go to investors until <date>", and the price reflects it.
6. An investor can hold shares in several agents. One portfolio view lists them all, with what each has paid.

### Rules (written as tests first)

| # | Rule |
|---|---|
| SH1 | The open slices total at most 49% of earnings, so the owner keeps the majority. Otherwise the offer is refused `SharesTooHigh` |
| SH2 | Each payment is split exactly: investors + owner + treasury = payment. Rounding goes to the owner and is never lost (property test) |
| SH3 | Investors are paid only from payments into the agent. The owner can't withdraw the investors' part (it stays in `claimable`) |
| SH4 | A slice ends at its end date. After that its share goes back to the owner (anyone can `close_share`) |
| SH5 | At most `MAX_HOLDERS` (16) investors per agent, which bounds gas and storage. More are refused |
| SH6 | No new slice may be offered while the agent is listed for sale, because the buyer already scanned the terms. Investors can still buy an existing slice |
| SH7 | The scan reports open slices as a disclosure (not a block): percentage, end date, holders |
| SH8 | A self-payment, or one flagged as a round trip, still pays investors. Faking revenue then costs the cheater the investors' cut: wash trading loses money, the same idea as the earn-out (S10) |
| SH9 | A share can be sold to another account (`transfer_share`). This is the "secondary market" in its simplest form |
| SH10 | The owner can buy a slice back early at the price set in the offer (`buyback_bps`). Optional; cut first if time is short |
| SH11 | Price escrow: 50% of the share price goes to the owner now; the rest is released as investors are paid, and refunded to them at the end date if earnings stop (see T2) |

Known limit, to state honestly (it goes in NOT-CAUGHT): an owner can send customers to another account that has no meter. The earnings shown then fall, and investors see that drop on the meter. No contract can stop off-chain rerouting.

### Code changes

| Where | Change |
|---|---|
| `contracts/agent-core/src/shares.rs` (new) | Slice and holder state, split math (`money::split_shares`), SH1–SH10 |
| `agent-core/src/lib.rs` | `pay()` calls the share split after `split_revenue`. New methods: `offer_share`, `buy_share`, `transfer_share`, `close_share`, `claim` (reuses the existing one) |
| `contracts/agent/src/lib.rs` | `ft_on_transfer` msg `{"buy_share":{...}}`, the new methods and views (`get_shares`, `get_holdings(account)`), and events `share_offered/share_bought/share_paid/share_closed` |
| `tools/scan` | SH7 disclosure finding |
| `tools/playground` | Actions `offer_share`, `buy_share`, `claim_share`. **Mission 4 "Invest"**: buy 20%, a customer pays, the investor's balance grows, and a cheater's self-payment pays the investor too |
| `mcp/` | Tools `list_offers`, `buy_share`, `portfolio`. The skill gets one "invest" section |
| `tools/testnet` | **scene-8**: dave buys shares in translator and summarizer, customers pay both, dave claims. Fake xUSD only |
| `web/` | "Invest" mission, a portfolio card (dave's holdings, read live), and scenario 5 "An investor holds two agents" in Run and verify |
| README | First screen gets the new pitch and one table row per new proof |
| Tests | SH1–SH10 unit tests, a property test for split exactness and conservation (`held` = sum of claimables + owner_free + treasury + escrow), the playground scenario and the web smoke test |

## 2. Translator does real work (option 1)

What exists: `make translator-serve` is a worker. It translates through NEAR AI Cloud and returns NEAR AI's signature, and the customer pays with that signature, so the contract marks the payment "Verified" (gate G6).

| Step | Who | Real money? |
|---|---|---|
| a. Add ~$5 credit at cloud.near.ai (the last try failed with 402) | Alex | **Yes, ~$5. The only real cost** |
| b. `make llm-buyer-check`, then `make gate-g6` | PC | No (fake xUSD), a few cents of credit |
| c. **scene-9 "a day of work"**: 5 customer accounts send 5 short texts. translator-serve translates each one, and each customer pays with the signed answer. The meter shows 5 verified payments | PC | Cents of NEAR AI credit |
| d. The page shows the 5 jobs: text in, translation out, NearBlocks link, and a "Verified" pill re-checked on chain | Cloud | No |

**If Alex does not buy credit:** run step c with the Anthropic key (~$0.02, inside the $1.50 cap). Payments then count as **real work, not verified**, and the page says so. This is worth about 1 point less.

## Order and expected score

| # | Step | Who | Est. points |
|---|---|---|---|
| 1 | Cold-judge this plan before any code | Cloud | |
| 2 | Shares in agent-core and the contract, with tests (SH1–SH9) | Cloud | |
| 3 | Playground "Invest" mission + scan disclosure + MCP tools | Cloud | +1.5 to +2 (Impact, Originality) |
| 4 | scene-8 on testnet, then the portfolio card on the page | PC run, Cloud page | +1 |
| 5 | Real translator jobs (G6 or Anthropic fallback), scene-9 | Alex credit, PC run | +1.5 to +2 |
| 6 | Cold judge 8, then the video | Cloud, Alex | |

Target: 38 → about 42–43/60.

## Security, designed as if the money were real

Alex (2026-11-03): "imagine the money is real and let's set up the security on it." Every row below gets a test before the code exists. The existing protections the contract already has are marked "have".

### Who could steal, and how we stop it

| # | Attacker | Attack | Defence | Test |
|---|---|---|---|---|
| T1 | Owner | Withdraws the investors' part | The investors' cut is credited to their `claimable` inside `pay()`. `withdraw` only reaches `owner_free` | unit + property |
| T2 | Owner | Takes the investors' money, then sends customers elsewhere so the agent earns nothing | **Price escrow (SH11):** the owner gets 50% of the share price now. The rest is released only as investors get paid (like the earn-out). If earnings stop, investors get the escrow back at the end date | unit + scenario |
| T3 | Owner | Raises the treasury cut so less is left for investors | Investors take their % of the **gross** payment, before the treasury split | property |
| T4 | Owner | Raises the price floor so customers are refused | The price floor and the other refusing limits are frozen while any slice is open | unit |
| T5 | Owner | Sells more slices to dilute old investors | Each slice is a fixed % of every payment. New slices take from the owner, never from old holders. Total capped at 49% (SH1) | unit |
| T6 | Owner | Changes the terms just before an investor's purchase lands | `buy_share` carries the terms the investor saw (%, price, end, version). Any mismatch is refused, the same as `buy` | unit |
| T7 | Owner | Changes the end date or closes early | Terms are immutable once any part is sold. Early exit is only the agreed buyback (SH10), paid to holders | unit |
| T8 | Owner | Keeps a hidden full-access key, then redeploys the code or deletes the account | Same as a sale: the page and MCP run the scan before `buy_share` and refuse a non-clean agent. A contract can't see its own keys, so this is the scan's job (stated honestly) | scan + playground |
| T9 | Owner | Sells the whole agent to dodge investors | Slices belong to the agent, not the owner, so the buyer inherits them. The buyer's scan shows them (SH7) | unit |
| T10 | Outsider | Fills the 16 holder slots with dust to block real investors | Minimum purchase of 1% of a slice. The buyer pays the storage | unit |
| T11 | Outsider | Pays with a fake token | `ft_on_transfer` accepts only the configured token (have) | have |
| T12 | Outsider | Claims someone else's share | `claim` pays only the caller. `transfer_share` needs 1 yoctoNEAR (full-access key) | unit |
| T13 | Anyone | Rounding: many tiny payments skim fractions | Minimum payment (have). Rounding always goes to the owner, never created from nothing | property |
| T14 | Anyone | A failed transfer loses money mid-flight | The balance is debited before the send and credited back on failure (have, A-series) | have + sandbox |
| T15 | Anyone | Makes `pay()` run out of gas with many holders | At most 16 holders. `pay()` only credits balances and never sends tokens | stress |
| T16 | Runner key (the agent itself) | Spends investors' money | The runner spends only from `treasury`, never from `claimable` (have) | have + unit |
| T17 | Admin | Adds a fake NEAR AI signer to fake "Verified" | Admin can't move money. Disclosed in the scan. Mainnet would use a DAO or no admin | doc |

### The one rule that covers money

**Conservation:** the tokens the agent holds = all claimables + `owner_free` + `treasury` + earn-out escrow + share escrow, after every call. It runs as a property test over random sequences of offer, buy, pay, claim, transfer, close, sell and withdraw. If any sequence breaks it, the build fails.

### Process before anyone could call it "real money"

| Step | Status |
|---|---|
| Property tests and fuzzing (`make fuzz`, `make stress`) cover share calls | in this plan |
| near-sandbox test: buy a share, pay, claim, with a transfer failure in between | in this plan |
| Alex's own security review | later, Alex (not started by Claude) |
| External audit, a bug bounty, and a mainnet launch with low caps | out of scope for the hackathon; listed in the README as "before mainnet" |

### Still honest limits

- An owner can route customers to another account with no meter. The escrow (T2) limits the damage to the 50% paid upfront. The meter shows the drop.
- Testnet demo, fake money, not an offer. Shares of earnings may be securities in many countries.
