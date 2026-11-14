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

The same author also entered Hire Tree (https://github.com/alexandergorokhov/hire-tree), a separate, independent project.

## X post (owner posts it, with the video attached)

Rules (HACKATHON.md Procedure 2): say it was built at NEAR HACKATHON, tag @NEARProtocol and #NEARHackathon, attach the video or screenshots. Optional tags: @NearLegion, @NearBuilders. The post's URL is the form's `demoUrl`.

> AI agents earn real money. But buying one is risky: the seller can fake its revenue, or keep a key and take it back.
>
> Built at NEAR HACKATHON: Agent Exit. One @NEARProtocol transaction gives the buyer the agent, its money and keys, and deletes the seller's key.
>
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
| `projectUrl` | `https://github.com/alexandergorokhov/agent-exit` (must be public by 2026-11-14 23:59 UTC) |
| `demoUrl` | URL of the X post above |
| `idea` | the text below (20-2000 characters) |
| `consent` | `true`, only after reading the contact notice |

Never fill `website` (spam trap). One submission per repository, and it cannot be edited.

`idea`:

> THE PROBLEM
> AI agents now earn real money, so people want to buy or invest in them, like small businesses. But a buyer can't check two things. First, revenue can be faked: the owner pays their own agent. Second, the seller can keep a secret key and use it after the sale to take the agent or its money back.
>
> WHAT WE BUILT
> Agent Exit: a market where you buy AI agents by what they really earn. Each agent is its own NEAR account.
> 1. Real revenue. The contract records who paid. Payments from the owner, from the owner's sub-accounts, or money the agent sent out that comes back are flagged and never counted. Payments from a wallet with no link to the owner can't be spotted. To limit that, due diligence counts new payers at half, NEAR AI-signed jobs are marked verified, and part of the price reaches the seller only as real customers keep paying, so a seller who faked revenue loses most of that part.
> 2. Invest. Buy a share of an agent's future earnings. It is credited to you inside every customer payment.
> 3. Buy. One transaction moves the money, the ownership token (NEP-171) and the keys, and deletes the seller's key.
> 4. Born clean. The contract refuses to list or sell any agent our factory did not create. The factory creates agents with no seller key.
> AI agents can buy too, via our MCP server, which forces a scan before payment.
>
> HOW NEAR FITS
> On NEAR, an agent can be an account, and keys are on-chain objects that can be added, limited and deleted. So the sale itself deletes the seller's key: after the sale, the seller can't sign for the agent or move its money, and anyone can check that on chain. NEAR AI Cloud signatures make real work checkable. Chain Signatures give each agent a Base wallet only its current owner can use.
>
> PROOF
> Live on NEAR testnet with test money (Base payments are signed, not yet sent). After a test sale, the seller's six attempts to get back in are real failed transactions. `make judge` re-checks the headline claims offline in about a second.
