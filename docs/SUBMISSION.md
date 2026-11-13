# Agent Exit: submission

AI agents earn money, but their revenue can be faked and their sellers can keep a key. Agent Exit checks both on NEAR, so an investor can buy a share of an agent's earnings, or the whole agent.

## Four proofs (NEAR testnet, all money xUSD-FAKE)

1. **Revenue counted by who paid.** A NEAR AI Cloud signature is checked by the contract ([verified job](https://testnet.nearblocks.io/txns/3Z29iLiZet2ydtqbEy63jM7nx621cosBx1rYYby1b7qY)); the owner's own payment is flagged, not counted ([flagged](https://testnet.nearblocks.io/txns/EvNByMd28t71hp6sFMrJvKnSwjc9gU5vNywSn1ki11Mk)).
2. **Invest.** dave buys 20% of an agent's earnings and claims exactly 20% ([claim](https://testnet.nearblocks.io/txns/B2L5ft8WL7qorpVD5GaFqshWvaCarxoAypHi56TeYUa7)).
3. **A keyed agent cannot be listed.** The contract refuses the cheat's listing (`NotBornClean`, [refused](https://testnet.nearblocks.io/txns/BQrAUGB6yMia6Ep4ZdTSWegoVgZqfP7LJPJV8Dnh99Pu)) and refunds a buy sent anyway ([refund](https://testnet.nearblocks.io/txns/2Rf95RJft2qHJbtJfXKYGihbnLjtFbpLinuL8wkgQP6H)).
4. **Exit.** One transaction sells the whole agent ([sale](https://testnet.nearblocks.io/txns/Hh9kTyAPxL8yMFAawj6rDtf2eGFtGZiXCKuRFnbwBkUs)); then 6 ways back in are refused on chain and her old key is gone at the node ([one of them](https://testnet.nearblocks.io/txns/5aYWbex9PRXeoNQBx7Pny9acwygBn2kmossSuGAukWuw)).

## Verify it

```bash
make setup && make mcp-build
make judge          # offline, real contract code
make judge-testnet  # read-only testnet re-read
```

In Claude Code, run the MCP prompt `/mcp__agent-exit__judge_tour`. Every claim and its link: `docs/PROOF.md`.

## Tracks

Autonomous Business, AI Security & Defense, NFTs That Do Something, Agent Economy.

## Other project (FAQ Q-015)

The same author also entered Hire Tree (https://github.com/alexandergorokhov/agent-hire), a separate, independent project.

## X post (owner posts it, with the video attached)

Rules (HACKATHON.md Procedure 2): say it was built at NEAR HACKATHON, tag @NEARProtocol and #NEARHackathon, attach the video or screenshots. Optional tags: @NearLegion, @NearBuilders. The post's URL is the form's `demoUrl`.

> Built at NEAR Hackathon: Agent Exit
>
> Buying an AI agent is risky: owners fake revenue, sellers keep a key.
>
> Agent Exit counts revenue by who paid, and one transaction hands over the agent and deletes the seller's key. @NEARProtocol testnet.
>
> https://near-transparent-agent-market-web.vercel.app/
> #NEARHackathon

Video: `https://near-transparent-agent-market-web.vercel.app/media/agent-exit-demo.mp4` (download and attach it to the post).

## Form fields (owner submits; the form or `POST /api/applications`, exactly these fields)

| Field | Value |
|---|---|
| `projectName` | `Agent Exit` |
| `leadName` | owner's name |
| `email` | owner's email (max 2 submissions per email; Hire Tree is the second) |
| `size` | `1` |
| `tracks` | `["business", "security", "nfts", "agents"]` |
| `nearAddress` | owner's **mainnet** payout account (`name.near` or 64-hex implicit) |
| `projectUrl` | `https://github.com/alexandergorokhov/near-transparent-agent-market` (must be public by 2026-11-14 23:59 UTC) |
| `demoUrl` | URL of the X post above |
| `idea` | the text below (20-2000 characters) |
| `consent` | `true`, only after reading the contact notice |

Never fill `website` (spam trap). One submission per repository, and it cannot be edited.

`idea`:

> The problem: AI agents now earn real money doing paid work, so people want to buy them or invest in them, like small businesses. But a buyer can't trust two things. Revenue can be faked: the owner simply pays their own agent. And the seller can keep a secret key and take the agent, or its money, back after the sale.
>
> What we built: Agent Exit, a market where you buy AI agents by what they really earn. Each agent is its own NEAR account running our contract.
> 1. Real revenue: the contract records who paid. Payments from the owner, the owner's sub-accounts or round trips are flagged and never count. Jobs signed by NEAR AI Cloud are verified on-chain. Part of every sale price is paid to the seller only as real customers keep paying, so faking revenue costs the seller money.
> 2. Invest: buy a share of an agent's future earnings, paid to you automatically inside every customer payment.
> 3. Buy the whole agent in one transaction: the money, the ownership token (NEP-171) and the keys move together, and the seller's key is deleted.
> 4. Born clean: the contract refuses to list or sell any agent that our factory did not create, so an agent that ever had a hidden key can't be listed or bought on the market.
> Agents can buy too, over MCP (13 tools; a scan is forced before payment).
>
> How NEAR fits: on NEAR an agent can be an account, and keys are on-chain objects that can be added, limited and deleted. That makes "the seller is locked out" a fact anyone can check, not a promise. NEAR AI Cloud signatures make real work checkable, and Chain Signatures give each agent a Base wallet that follows the owner: after a sale, only the new owner can make it sign.
>
> Proof: the market runs on NEAR testnet with test money (Base payments are signed but not yet sent on Base). After the sale, the old seller's attempts to get back in are real failed transactions. Run `make judge` to re-check the headline claims against the real contract code in about a second, or press Run and verify on the page.
