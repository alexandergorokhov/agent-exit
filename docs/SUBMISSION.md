# Agent Exit: submission

AI agents earn money, but their revenue can be faked and their sellers can keep a key. Agent Exit checks both on NEAR, so an investor can buy a share of an agent's earnings, or the whole agent, safely.

## Three proofs (NEAR testnet, all money xUSD-FAKE)

1. **Revenue counted by who paid.** A NEAR AI Cloud signature is checked by the contract ([verified job](https://testnet.nearblocks.io/txns/GTWFcGyXi3RN6wQL3ZSPcHmP1E1aBHzyUUeaVT4BKudb)); the owner's own payment is flagged, not counted ([flagged](https://testnet.nearblocks.io/txns/HfdsWPaTpgMkQmb3biHCJQAMKd96cWXxiaRSoRbJDk21)).
2. **Invest.** dave buys 20% of an agent's earnings and claims exactly 20% ([claim](https://testnet.nearblocks.io/txns/Evd1jGnUhTtdbSJ2uSsf2aSMMgtJLtiN153Uqpk1Fajy)).
3. **Exit.** One transaction sells the whole agent ([sale](https://testnet.nearblocks.io/txns/oSpSKG9KHLdQXfBDfV2RXDH4EFpp7Toi8PRgB5nZLsC)); then all 6 ways back in fail ([one of them](https://testnet.nearblocks.io/txns/FixXD2tyGbjKPrjXvL5UsjY3482n8oJWTPCcSD5NRTk6)).

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
