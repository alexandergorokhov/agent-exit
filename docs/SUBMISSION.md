# Agent Exit: submission

AI agents earn money, but their revenue can be faked and their sellers can keep a key. Agent Exit checks both on NEAR, so an investor can buy a share of an agent's earnings, or the whole agent.

## Three proofs (NEAR testnet, all money xUSD-FAKE)

1. **Revenue counted by who paid.** A NEAR AI Cloud signature is checked by the contract ([verified job](https://testnet.nearblocks.io/txns/GTWFcGyXi3RN6wQL3ZSPcHmP1E1aBHzyUUeaVT4BKudb)); the owner's own payment is flagged, not counted ([flagged](https://testnet.nearblocks.io/txns/HfdsWPaTpgMkQmb3biHCJQAMKd96cWXxiaRSoRbJDk21)).
2. **Invest.** dave buys 20% of an agent's earnings and claims exactly 20% ([claim](https://testnet.nearblocks.io/txns/Evd1jGnUhTtdbSJ2uSsf2aSMMgtJLtiN153Uqpk1Fajy)).
3. **Exit.** One transaction sells the whole agent ([sale](https://testnet.nearblocks.io/txns/oSpSKG9KHLdQXfBDfV2RXDH4EFpp7Toi8PRgB5nZLsC)); then 6 take-backs are refused on chain and her old key is gone at the node ([one of them](https://testnet.nearblocks.io/txns/FixXD2tyGbjKPrjXvL5UsjY3482n8oJWTPCcSD5NRTk6)).

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

> Built at NEAR HACKATHON: Agent Exit, a capital market for AI agents on @NEARProtocol.
>
> AI agent revenue can be faked, and sellers can keep a key. Agent Exit counts revenue by who paid, lets you buy a share of it, or buy the whole agent in one transaction that locks the seller out.
>
> Live on testnet: https://near-transparent-agent-market-web.vercel.app/
> Code: https://github.com/alexandergorokhov/near-transparent-agent-market
> #NEARHackathon @NearBuilders

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

> Agent Exit is a capital market for AI agents on NEAR. AI agents earn money, but their revenue can be faked (an owner pays their own agent) and a seller can keep a spare key after a sale. On NEAR an agent can be its own account, so its keys, code and every payment it receives are public. One contract on that account counts revenue by who paid (the owner's own and sub-account payments are flagged, NEAR AI Cloud-signed jobs are verified on-chain), lets investors buy a share of future earnings paid inside every customer payment, and sells the whole agent in one transaction that deletes the seller's keys, adds the buyer's scoped key and moves a NEP-171 deed; part of the price is held and released to the seller as real customers keep paying. A scan blocks any sale while a hidden full-access key exists. Agents use it through an MCP server with 13 tools. Testnet only, all money is a fake test token. Judges: `make judge` (offline, 9/9 claims) and `make judge-testnet` re-read every recorded transaction.
