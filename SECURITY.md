# Security

Agent Exit runs on NEAR **testnet** with fake money (`xUSD-FAKE`). Do not use it with real funds.

## Report a problem

Open a GitHub issue titled "security:" with the scenario, or a private security advisory on this repository if the issue would let someone take an agent, its treasury or an investor's escrow. We answer within 3 days during the hackathon.

## What is in scope

- `contracts/agent`, `contracts/agent-core`, `contracts/xusd`
- the buyer's scan (`tools/scan`) and the MCP server (`mcp/`)
- the demo page's relay (`web/api`)

## What we already know

- Not caught by design: [docs/NOT-CAUGHT.md](docs/NOT-CAUGHT.md)
- Every attack we tested and its regression test: [docs/SCENARIOS.md](docs/SCENARIOS.md)
- Threat model and trust assumptions (MPC network, NEAR AI signer, RPC providers): see `docs/PROOF.md` and the review reports

Five adversarial review rounds have been run by the author; there has been no external audit yet ([docs/ROADMAP.md](docs/ROADMAP.md)).
