# Shares of future earnings: rules and threats

How investing in an agent works, the rules SH1-SH11 (each one a test) and the threats T1-T17. Tests: `contracts/agent-core/tests/shares.rs`, `contracts/agent/src/shares_tests.rs`, `mcp/test/shares_tools.test.ts`. All money is `xUSD-FAKE`.

## How it works

1. The owner offers a slice, for example "20% of earnings for 12 months, 500 xUSD". They choose the percentage, the price and the end date.
2. An investor buys it with one `ft_transfer_call`. The price goes to the owner. The investor does not need to buy the whole slice.
3. Each customer payment into the agent splits automatically: 20% to the investors, pro rata, credited as claimable, and the rest to the owner. The split runs inside `pay()`, so the owner cannot skip it.
4. The investor claims their share at any time.
5. When the agent is sold, its shares come with it. The buyer's scan shows "20% of earnings go to investors until <date>", and the price reflects it.
6. An investor can hold shares in several agents. One portfolio view lists them all, with what each has paid.

## Rules (written as tests first)

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
| SH9 | (not built) A share can be sold to another account (`transfer_share`). This is the "secondary market" in its simplest form |
| SH10 | (not built) The owner can buy a slice back early at the price set in the offer (`buyback_bps`). Optional; cut first if time is short |
| SH11 | Price escrow: 50% of the share price goes to the owner now; the rest is released as investors are paid, and refunded to them at the end date if earnings stop (see T2) |

Known limit, to state honestly (it goes in NOT-CAUGHT): an owner can send customers to another account that has no meter. The earnings shown then fall, and investors see that drop on the meter. No contract can stop off-chain rerouting.

## Security, designed as if the money were real

Designed as if the money were real. Every row below got a test before the code existed. Protections the contract already had are marked "have".

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
| Property test of conservation over random share sequences | done (`contracts/agent-core/tests/shares_prop.rs`) |
| Shares on a real chain: buy, pay, claim on NEAR testnet | done (scene 8) |
| An independent security review of the share code | before any real money |
| External audit, a bug bounty, and a mainnet launch with low caps | out of scope for the hackathon; listed in the README as "before mainnet" |

### Still honest limits

- An owner can route customers to another account with no meter. The escrow (T2) limits the damage to the 50% paid upfront. The meter shows the drop.
- Testnet demo, fake money, not an offer. Shares of earnings may be securities in many countries.

## As built (after adversarial review 1, 2026-11-04)

Where the code differs from the plan above, the code wins:

| Item | As built |
|---|---|
| SH6 | No new slice **and no share purchase** while the agent is listed or settling (`SaleInProgress`): the buyer's scan must stay true until the sale lands |
| SH8 / SH11 | A flagged payment (owner, sub-account, round trip) still pays investors, but **releases no price escrow** (review M2). Faking revenue now costs the cheater the investors' cut with nothing unlocked |
| T4 | The price floor cannot be raised while **any slice is open**, sold or not, so the owner cannot front-run the first investor (review M1) |
| T9 | When the deed moves (sale or transfer), the **unsold rest of every slice closes**; sold parts stay and are disclosed (review H1) |
| T10 | Minimum part is a tenth of the slice (`MIN_PART_DIV`), not 1%. No storage deposit is taken; the holder cap (16) bounds storage |
| T8 | The page and the investor scan (`scan_investor`) check the agent before buying a share. The MCP server's `buy_share` runs the same investor scan (`scan --investor`) and refuses a non-clean agent; `list_offers` and `portfolio` are read-only (2026-11-04, `mcp/test/shares_tools.test.ts`; offline replay of results recorded from the contract, not yet run on testnet) |
| T14 | No sandbox test with a transfer failure between buy, pay and claim yet (not built) |
| T15 | No `pay()` gas stress test with 16 holders yet (not built); `pay()` only credits balances |
| T17 | The admin's signer list is **not** read by the scan. "Verified" proves a NEAR AI inference happened for that payer, job and amount; it does not prove the payer is an independent customer (see NOT-CAUGHT N7) |
| Slots | `MAX_OPEN_OFFERS` counts only the current owner's own slices; inherited sold slices stay, shown at the price actually paid (review 2) |
| Deed moves | Every deed move trims unsold slices, including a transfer to the owner's own second account (documented, not prevented) |
| MCP | `buy_agent` refuses an agent with more than `AGENT_EXIT_MAX_SHARES_BPS` (default 25%) of earnings pledged (review H2); otherwise the result carries `shares_pct` and the `shares_disclosed` warning, and `list_agents` shows `shares_pct` (review 2 M2) |
