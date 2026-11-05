# Pitch and demo explanation

For the submission form, the video voice-over and a judge who has two minutes.
Order (owner's choice): **business → proven earnings → invest or buy → security**.
All money is `xUSD-FAKE` on NEAR testnet. Every claim below links to a test or a testnet transaction in the README.

## One line

Agent Exit turns an AI agent into a business you can trust: its earnings are proven on-chain, you can buy a share of them, and you can buy the whole agent without the seller keeping a way back in.

## 30 seconds

AI agents already do paid work. They will be bought, sold and invested in, much like small online businesses are today. Two things stop that: nobody can trust the revenue numbers, and nobody can trust that the seller has really let go of the agent.

On NEAR an agent can be its own account. Agent Exit puts a contract on that account. The contract counts every payment by who paid it, so the owner paying himself is flagged and not counted. Work signed by NEAR AI Cloud is checked on-chain and marked verified. Those proven earnings unlock money: investors buy a percentage of future earnings and claim it pro rata. A buyer can also buy the whole agent. One transaction moves the deed, deletes the seller's key and adds the buyer's. The seller gets 70% now and 30% from real customer payments after the sale. A scanner blocks any agent with a hidden key before money moves. After the sale, every way back in for the seller fails on-chain.

## Two-minute demo walkthrough (the page)

Open https://near-transparent-agent-market-web.vercel.app/. It needs no wallet and no setup.

| # | Do | See | Why it matters |
|---|---|---|---|
| 1 | **Try it yourself → mission "1. Fake the revenue"**: press the buttons in order | The owner pays the agent 200; the meter records it as flagged, not counted. A stranger's 25 counts. "Behind the scenes" names the contract check. | Revenue you can trust: counted by payer, on-chain. |
| 2 | **The same story on NEAR testnet → "Invest in an agent: 20% of its earnings" → Run and verify** | Dave buys 20% of an agent's earnings. A customer pays for a real NEAR AI translation, and dave claims exactly 20%. A payment from the owner's sub-account releases nothing. Each stage turns "confirmed on chain" with a NearBlocks link. | A new market: shares in agents, paid only from real revenue. |
| 3 | **"A cheat is caught before the sale" → Run and verify** (or mission "2. Hide a backdoor") | The scan finds a hidden full-access key and the buy is refused; no money moves. | Buyer protection comes before payment, not after. |
| 4 | **"Sell an agent, lock the seller out" → Run and verify** | First the sale: one transaction moves the deed, deletes the seller's key and adds the buyer's; 70% is paid now and 30% is held in escrow. Then the break-in: the old seller withdraws, adds a key, changes the limits, moves the deed and spends from the Base wallet. Each attempt fails on-chain, and her old key no longer exists. | "Sold" means locked out, proven by real failed transactions. |
| 5 | **"A real AI model buys an agent through MCP"** | Claude Haiku, given only our MCP tools and Skill, refused the cheat, bought the honest agent on testnet and checked that the seller is locked out, for about 3 cents. | Agents can be the buyers too. |

**What "Run and verify" does:** the page asks a NEAR RPC node for each recorded transaction right now, through a read-only relay. It compares the signer, the method, the outcome and the contract's event logs with the recording, and marks a stage confirmed only if they match. It does not replay a video. To check without our relay, open the NearBlocks link next to each step.

## How it works behind the page

- **Agent account = contract** (`contracts/agent`). It holds the earnings ledger, the spending limits (daily cap, allowed payees), the shares and the escrow. Payments come in as NEP-141 `ft_transfer_call`.
- **Deed** is a NEP-171 token. Owning the deed means owning the agent.
- **Handover**: one batch on the agent's own account deletes the seller's runner key and adds the buyer's, then the `on_handover` callback moves the deed. If the batch fails, the buyer is refunded.
- **Verified revenue**: the NEAR AI Cloud signature over `model:sha(request):sha(response)` is checked with `ecrecover` against an admin-set signer list. Each job is single-use.
- **Base wallet** through NEAR Chain Signatures. It is derived from the agent's account name, so it changes hands with the deed, and the old seller's attempt to spend from it is refused.
- **Scanner** (`tools/scan`). It checks the code hash against a reproducible build, the methods, every access key, the pending key changes and the Base wallet. It uses two RPC providers. Any doubt blocks the buy.
- **For AI agents**: an MCP server with 9 tools and an Agent Skill. The server, not the model, enforces `confirm`, `max_price`, a daily spend cap and an operator allowlist. Chain text is treated as data and sanitised.

## Why only on NEAR

1. **Keys are first-class and deletable**, so "the seller has no key left" is a fact the chain can prove. That is not true of an EOA, where a private key can always be copied.
2. **Function-call access keys** let the agent's runner do its job without being able to drain or re-key the account.
3. **Chain Signatures** give the agent a Base wallet that belongs to whoever owns the NEAR account. No bridge, and nothing for the seller to keep.
4. **NEAR AI Cloud** signs inference inside a TEE, and the contract checks that signature on-chain, which turns "the agent did real work" into data.

## Judge FAQ

- **Is this just a web game?** No. The page is a viewer. Contracts are deployed on testnet (code hash `Aots3gb3…`, the same as the reproducible build), and every step links to a real transaction. `make verify` re-runs 232 tests offline. `make check-links` checks that all 37 transaction links exist.
- **Can I run it?** `make setup && make demo` runs everything offline in a few minutes, with no keys. `make verify` and `make attack` show the tests and the 19 attack families.
- **What can't it catch?** It cannot catch a payer who is a real stranger paid off-chain, or a seller who leaks the agent's off-chain secrets (prompts, API keys). The README section "Honest limits" and rows N1-N7 in every `make attack` run list them. "Verified" means a NEAR AI job really ran; it does not prove that the payer is independent.
- **Is the money real?** No. Everything is `xUSD-FAKE` on testnet. No mainnet deployment yet.

## Close (the last 10 seconds of the video)

"Open source, MIT. Press Run and verify on the page, or run make verify, to check every step yourself."
