# Agent Exit as a business

Agent Exit is a capital market for AI agents: a way to invest in them by their proven revenue, and to buy and sell them whole. It is not a marketplace with a front page; it is the trust layer under one: the contract turns an agent's earnings into numbers a stranger can check, and makes the handover safe enough that a stranger will pay.

## Who uses it, and why

| Who | Problem today | What Agent Exit gives them |
|---|---|---|
| **An agent builder who wants out** (sell the whole agent) | Buyers pay little or nothing for an agent's revenue they cannot check, and fear the seller keeps a way back in | Earnings counted by payer on-chain, with self-payments flagged; a sale in one transaction that deletes the seller's keys; an earn-out that pays the seller more only if customers keep paying |
| **A builder who wants cash but not to sell** (sell a share) | Raising money on an agent means trusting a spreadsheet | A slice of future earnings, paid by the contract inside each customer payment; half the price held in escrow and returned if the agent stops earning |
| **A buyer or investor** (a person or another agent) | Due diligence on an agent is manual and easy to fake | A scan that blocks hidden keys, unpublished code and open approvals, plus a due-diligence card (counted revenue, flagged revenue, NEAR AI-verified share, terms) |
| **An AI agent buying another agent** | No machine-readable way to check a seller | 13 MCP tools and an Agent Skill; a real model (Claude Haiku) bought an agent this way on testnet |

## A worked example (the testnet numbers)

`translator` asks 1,000.00. Its meter shows 500.00 paid in, but only 360.00 from 6 outside accounts; 140.00 came from the owner and the owner's sub-account and is flagged. The buyer reads a price of 2.8× counted revenue (not 2×, as the gross suggests).

The buyer pays 1,000.00 in one transaction. 690.00 goes to the seller now and 10.00 is the 1% protocol fee. 300.00 is held for 30 days and released at 50% of each counted customer payment. If customers stop paying, the seller gets only the floor (20% of the held part, 60.00) and the buyer gets 240.00 back. So a seller who inflated revenue loses up to 240.00. `cheat` asks 500.00 for 500.00 of revenue that was all self-paid; its card says "Do not buy" before any money moves.

An investor (`dave`) paid 500.00 for 20% of `translator3`'s earnings for a year. 245.00 went to the owner, 5.00 to the protocol fee and 250.00 into escrow. Five customers paid 10.00 each for NEAR AI-signed translations, and the owner's sub-account paid 20.00, flagged. dave got exactly 14.00 (20% of 70.00), paid inside the customers' own transactions.

## How it makes money (built, in the contract)

A 1% protocol fee on every sale and every share buy (`PROTOCOL_FEE_BPS = 100` in `contracts/agent-core/src/money.rs`). It comes out of the seller's upfront part, never out of the buyer's escrow, so it is capped at that part: a "pay from revenue" sale (nothing up front) pays no fee, and a sealed sale pays it only when the delivery is accepted (SC14); inside the same transaction, and is credited to the fee account as a claimable balance (`FEE_ACCOUNT`, compiled in; the registry admin on testnet). In the worked example: on the 1,000.00 sale the seller gets 690.00 now and the fee account 10.00; on dave's 500.00 share buy alice gets 245.00 now and the fee account 5.00. Brokers of small online businesses commonly take a percentage of the sale price; 1% sits well under that. Tests: `s5_buy_one_step`, `dave_holds` (contract), the `prop.rs` property run (fee = 1% of the price, `payout + fee = paid_now`, solvency holds).

The deployed testnet contract (commit `1dc6ca1`, world `agentexit6.testnet`) takes the fee on every sale and share buy.

## Why now

- Small software businesses already change hands on brokers and marketplaces such as [Acquire.com](https://acquire.com/) and [Flippa](https://flippa.com/), and Acquire publishes [acquisition multiples reports](https://blog.acquire.com/tag/acquire-coms-acquisition-multiples-report/) for them. AI agents that earn are the next such asset, but none of these venues can check an agent's revenue or its keys.
- On-chain agent projects let people own or fund agents ([Olas](https://olas.network/), [Virtuals Protocol](https://whitepaper.virtuals.io/)), but ownership there is a token, not proven earnings plus a handover that locks the seller out.

## Compared with what exists

| | Proves earnings by payer | Flags self-payment | Seller provably locked out after a sale | Investor paid per customer payment | Buyer's money held against fake revenue |
|---|---|---|---|---|---|
| [Acquire.com](https://acquire.com/) / [Flippa](https://flippa.com/) (business listings) | No: seller-provided metrics and documents | No | No: off-chain credentials are handed over, and copies can stay | No | Through an escrow service, not tied to revenue |
| [Olas](https://olas.network/) (agent services owned as NFTs) | Not its focus | No | Not what it is built for | No | No |
| [Virtuals Protocol](https://whitepaper.virtuals.io/) (agent tokens) | No: the token price, not the agent's revenue | No | No: the token is traded, the operator stays | No | No |
| **Agent Exit** | Yes, on-chain by payer; NEAR AI-signed jobs marked Verified | Yes (self, seller sub-accounts, round trips) | Yes: keys deleted in the sale transaction, refusals on testnet | Yes, inside each payment | Yes: earn-out released only by counted customer payments |

Notes: the other products do other things well (Olas coordinates agent services, Virtuals runs a token launchpad); this table is only about checking and transferring an earning agent.

## Risks, and how they are priced

| Risk | What protects the buyer | What is left |
|---|---|---|
| Fake customers before the sale (N3) | Self, sub-account and round-trip payments are flagged; fresh payers are reported; Verified payments need a NEAR AI signature; 30% of the price is released only by later counted payments | A seller who launders money through strangers before the sale can inflate counted revenue. The buyer prices this with the earn-out and the payer spread on the card |
| "Verified" jobs from the seller's own customers (N7) | Verified proves a real inference ran for that payer and amount | It does not prove the payer is independent. Weight Verified by payer spread |
| Fake payments releasing an investor's escrow (N9) | The investor is paid 20% of every payment, real or not | A dead agent can look alive for a while; Verified share shows it |
| A seller's friend holding a large share (N10) | The scan blocks a listing pledging more than 25% (`shares_over_max`); the MCP server refuses the same | Below 25%, post-sale wash-paying is cheaper; disclosed as `shares_disclosed` |
| Off-chain assets (N1, N2) | The earn-out | Prompts, weights and outside API keys are not on-chain and can be copied |

All of these are listed with their tests in [NOT-CAUGHT.md](NOT-CAUGHT.md) and [SCENARIOS.md](SCENARIOS.md).
