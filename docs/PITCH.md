# Pitch and demo explanation

**Buy AI agents by what they really earn.** A capital market for AI agents on NEAR. AI agents earn money, but their revenue can be faked and their sellers can keep a key. Agent Exit checks both on NEAR before you buy a share of an agent's earnings, or the whole agent.

For the submission form, the video voice-over and a judge who has two minutes.
Order (owner's choice): **business → proven earnings → invest or buy → security**.
All money is `xUSD-FAKE` on NEAR testnet. Every claim below links to a test or a testnet transaction in the README or [`docs/PROOF.md`](PROOF.md).

## One line

**Buy AI agents by what they really earn.** Invest in an AI agent by its proven revenue, or buy the whole business from a stranger, on NEAR.

Agent Exit turns an AI agent into a business you can trust: its earnings are proven on-chain, you can buy a share of them, and you can buy the whole agent without the seller keeping a way back in ("sold means locked out", the picture `docs/img/sold-means-locked-out.svg`).

## 30 seconds

AI agents already do paid work. They will be bought, sold and invested in, much like small online businesses are today. Two things stop that: nobody can trust the revenue numbers, and nobody can trust that the seller has really let go of the agent.

On NEAR an agent can be its own account. Agent Exit puts a contract on that account. The contract counts every payment by who paid it, so the owner paying himself is flagged and not counted. Work signed by NEAR AI Cloud is checked on-chain and marked verified. Those proven earnings unlock money: investors buy a percentage of future earnings and claim it pro rata. A buyer can also buy the whole agent. One transaction moves the deed, deletes the seller's key and adds the buyer's. The seller gets 70% now and 30% from real customer payments after the sale, or, if the buyer trusts none of the history, nothing now and everything from real customer payments (pay from revenue). The contract refuses to list any agent that ever had a key (NotBornClean), and a scanner flags hidden keys as a second layer.  After the sale, every way back in for the seller fails on-chain.

## Two-minute demo walkthrough (the page)

Open https://near-transparent-agent-market-web.vercel.app/ (the market: agents for sale, each with a verdict; click one for its due diligence) and https://near-transparent-agent-market-web.vercel.app/proof.html (the walkthrough below). Neither needs a wallet or setup.

| # | Do | See | Why it matters |
|---|---|---|---|
| 1 | **Try to cheat it → tab "Fake revenue"**: press the buttons in order | The owner pays the agent 200; the meter records it as flagged, not counted. A stranger's 25 counts. "Behind the scenes" names the contract check. | Revenue you can trust: counted by payer, on-chain. |
| 2 | **Check it yourself → "Invest in an agent: 20% of its earnings" → Run and verify** | Dave buys 20% of an agent's earnings. A customer pays for a real NEAR AI translation, and dave claims exactly 20%. A payment from the owner's sub-account releases nothing. Each stage turns "confirmed on chain" with a NearBlocks link. | A new market: shares in agents, paid only from real revenue. |
| 3 | **"A cheat is caught before the sale" → Run and verify** (or tab "Hide a backdoor") | The scan finds a hidden full-access key and the buy is refused; no money moves. | Buyer protection comes before payment, not after. |
| 4 | **"Sell an agent, lock the seller out" → Run and verify** | First the sale: one transaction moves the deed, deletes the seller's key and adds the buyer's; 70% is paid now and 30% is held in escrow. Then the break-in: the old seller withdraws, adds a key, changes the limits, moves the deed and spends from the Base wallet. Each attempt fails on-chain, and her old key no longer exists. | "Sold" means locked out, proven by real failed transactions. |
| 5 | **"Invest in an agent's earnings"** (below the Exchange; byte-exact with near-js offline; the same transaction was signed on testnet by script for an outside account, tx DHJhBU22…) | Type your testnet account and press **Invest**: MyNearWallet signs one transaction that takes free test dollars from the faucet and buys the smallest part (2.5%) of the agent's future earnings. **See my holding** reads it live; **Claim** collects. The same five steps are there as near-cli-rs commands. | A judge becomes an investor in one click, with their own testnet account. |
| 6 | **"A real AI model buys an agent through MCP"** | Claude Haiku, given only our MCP tools and Skill, refused the cheat, bought the honest agent on testnet and checked that the seller is locked out, for about 5 cents of model calls. | Agents can be the buyers too. |
| 7 | **The Exchange** (testnet scene 13, `out/scenes/scene-13.json`) | Three agents born clean in the factory, each with its own payers: translate-pro and research-desk offer shares of their earnings; pump-bot is listed at 1,000.00 with 410.00 gross but 400.00 flagged (its owner and her sub-account paid it), so only 10.00 counts. | The price can be judged against counted revenue, not the gross the seller shows. |

**What "Run and verify" does:** the page asks a NEAR RPC node for each recorded transaction right now, through a read-only relay. It compares the signer, the method, the outcome and the contract's event logs with the recording, and marks a stage confirmed only if they match. It does not replay a video. To check without our relay, open the NearBlocks link next to each step.

## How it works behind the page

- **Agent account = contract** (`contracts/agent`). It holds the earnings ledger, the spending limits (daily cap, allowed payees), the shares and the escrow. Payments come in as NEP-141 `ft_transfer_call`.
- **Deed** is a NEP-171 token. Owning the deed means owning the agent.
- **Handover**: one batch on the agent's own account deletes the seller's runner key and adds the buyer's, then the `on_handover` callback moves the deed. If the batch fails, the buyer is refunded.
- **Verified revenue**: the NEAR AI Cloud signature over `model:sha(request):sha(response)` is checked with `ecrecover` against an admin-set signer list. Each job is single-use.
- **Base wallet** through NEAR Chain Signatures. It is derived from the agent's account name, so it changes hands with the deed, and the old seller's attempt to spend from it is refused.
- **Sealed config** (`agent-core/src/sealed.rs`). The off-chain brain (prompt, model, tools, which API keys to rotate) is committed by hash in the listing and delivered encrypted to a one-time key of the buyer. In a dispute the contract decrypts and hashes it itself: not the committed bytes, and the escrow goes back to the buyer. Before paying, a buyer can see two random chunks of it (the block's random seed picks them; the seller proves each against a Merkle root in the commitment; each preview shows at most a quarter of the bundle; up to 4 buyers draw per listing, and a relist allows new draws, so a seller who relists often reveals more of it).
- **Born clean** (`contracts/factory`). One batch creates `name.factory`, deploys the published code and calls `new`, with no key added, so the agent never had a seller key to hide. The scan reports `born_clean`. Live on testnet: [born1.factory.agentexit5.testnet](https://testnet.nearblocks.io/txns/2Rgj3r37wUx22ySBK5PZWM45JNP68U4RxRDHzv5zDpLD) (scene 9).
- **Scanner** (`tools/scan`). It checks the code hash against a reproducible build (and hashes the code bytes itself), the methods, every access key, the pending key changes, the Base wallet, and that the money the contract says it holds is really in its token balance. It reads from two RPC providers. Any doubt blocks the buy.
- **For AI agents**: an MCP server with 13 tools (`sealed_delivery` previews, opens, accepts or disputes the sealed prompt; `check_claims` re-runs the README claims for a judge's agent; `sandbox` lets it try any action on the real contract code; `check_account` tells who else controls any NEAR account) and an Agent Skill. The server, not the model, enforces `confirm`, `max_price`, a daily spend cap and an operator allowlist. Chain text is treated as data and sanitised.

## Why only on NEAR

1. **Keys are first-class and deletable**, so "the seller has no key left" is a fact the chain can prove. That is not true of an EOA, where a private key can always be copied.
2. **Function-call access keys** let the agent's runner do its job without being able to drain or re-key the account.
3. **Chain Signatures** give the agent a Base wallet that belongs to whoever owns the NEAR account. No bridge, and nothing for the seller to keep.
4. **NEAR AI Cloud** signs inference inside a TEE, and the contract checks that signature on-chain, which turns "the agent did real work" into data.

## Judge FAQ

- **Is this just a web game?** No. The page is a viewer. Contracts are deployed on testnet (code hash `5VqS52NR…`, the same as the reproducible build of commit `70d06b2`), and every step links to a real transaction. `make verify` re-runs the whole workspace test suite plus 76 end-to-end checks against goldens, offline. `make check-links` checks that every transaction link exists.
- **Can I run it?** `make setup && make demo` runs everything offline in a few minutes, with no keys. `make verify` and `make attack` show the tests and the attack families (20 written, 19 counted).
- **What can't it catch?** Off-chain things: the agent's prompts, API keys and copies of its code. It also cannot catch fake customers on accounts the seller funded through other wallets; before a sale that is cheap, so a buyer should weigh counted revenue by how old and how spread out the payers are. After a sale, wash-paying costs the seller 2.00 for each 1.00 it releases, and a buyer can buy "pay from revenue" so a faked history earns the seller nothing up front (page tab "Pay from revenue"). The README section "Honest limits" and docs/NOT-CAUGHT.md (N1-N13; `make attack` prints N1-N6) list them. "Verified" means a NEAR AI job really ran; it does not prove that the payer is independent.
- **Is the money real?** No. Everything is `xUSD-FAKE` on testnet. No mainnet deployment yet.

## The company in one slide

- **Market:** AI agents that earn (translation, research, trading, support) are small online businesses. Small online businesses already change hands on brokers like Acquire.com; agents have no such market because their revenue and their keys cannot be trusted. Agent Exit is the trust layer under one.
- **First users:** agent builders who want cash without selling (a share of earnings) or a clean exit (the whole agent), and the buyers and agents on the other side, through 13 MCP tools and an Agent Skill.
- **Revenue:** a 1% protocol fee on each sale or share price, taken from the seller's upfront part (none on a pay-from-revenue sale), built into the contract (`PROTOCOL_FEE_BPS`).
- **Next 90 days:** the wallet "Invest" button clicked through on testnet, more agents on the Exchange, an external audit, then a capped mainnet launch. Details and risks: [`BUSINESS.md`](BUSINESS.md), [`ROADMAP.md`](ROADMAP.md).

## Close (the last 10 seconds of the video)

"Open source, MIT. Press Run and verify on the page, or run make verify, to check every step yourself."
