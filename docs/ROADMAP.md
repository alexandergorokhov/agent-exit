# Roadmap

Agent Exit is a testnet prototype with fake money (`xUSD-FAKE`). Mainnet waits on every gate below.

| When | Gate | Status |
|---|---|---|
| Nov 2026 (hackathon) | Contract, scan, MCP server and page live on NEAR testnet; one-transaction sale, lockout, shares and NEAR AI-verified revenue recorded | Done |
| Nov 2026 | Review 5 contract fixes (C27–C32) redeployed on testnet; Base Sepolia payment through Chain Signatures (G2) | In progress |
| Nov 2026 | Protocol fee: 1% of each sale and share buy, from the seller's upfront part, credited to the fee account in the same transaction, with tests | Built; live on testnet after the next deploy |
| Q1 2027 | External security audit of `contracts/agent` and `contracts/agent-core`; public bug bounty on testnet | Planned |
| Q1 2027 | Code-change history check in the scan (closes N12), payer-age weighting on the due-diligence card (N3) | Planned |
| Q2 2027 | Mainnet with a real stablecoin, the mainnet MPC signer and NEAR AI signers; a cap on sale size until the audit has run for a quarter | Planned |
| Q2 2027 | A listing page and wallet buy button (today the page only reads the chain) | Planned |

Not planned: custody of anyone's funds outside the agent's own contract; an off-chain order book.
