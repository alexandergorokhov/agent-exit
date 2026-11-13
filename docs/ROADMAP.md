# Roadmap

Agent Exit is a testnet prototype with fake money (`xUSD-FAKE`). Mainnet waits on every gate below.

| When | Gate | Status |
|---|---|---|
| Nov 2026 (hackathon) | Contract, scan, MCP server and page live on NEAR testnet; one-transaction sale, lockout, shares and NEAR AI-verified revenue recorded | Done |
| Nov 2026 | Review 5 contract fixes (C27–C32) redeployed on testnet; Base Sepolia payment through Chain Signatures (G2) | In progress |
| Nov 2026 | Protocol fee: 1% of each sale and share buy, from the seller's upfront part, credited to the fee account in the same transaction, with tests | Built; live on testnet (2026-11-13, `agentexit6.testnet`) |
| Q1 2027 | External security audit of `contracts/agent` and `contracts/agent-core`; public bug bounty on testnet | Planned |
| Nov 2026 | Payer age and spread on the due-diligence card and in MCP `get_earnings` (`revenue_quality`): revenue from payers first seen in the last 7 days counts half (part of N3) | Built |
| Nov 2026 | Born-clean factory (`contracts/factory`): agents created in one batch with no access key and the published code, so N4 and N12 cannot happen to them; the scan reports `born_clean` | Built; live on testnet (2026-11-13, `agentexit6.testnet`, scene 9: `born1.factory.agentexit6.testnet`; the cheat's listing refused `NotBornClean`, scene 4) |
| Nov 2026 | Pay from revenue: a listing may hold the whole price, so the seller is paid only by customer payments after the sale; the due-diligence card shows the cost to fake the revenue | Built; live on testnet (2026-11-13, `agentexit6.testnet`) |
| Nov 2026 | Sealed agent config: the off-chain brain (prompt, tools, keys to rotate) sold in an on-chain fair exchange; on-chain decryption in a dispute | Built; live on testnet (2026-11-13, `agentexit6.testnet`) |
| Q1 2027 | Code-change history check in the scan (closes N12 for agents not born in the factory); account age from an indexer for older sybil accounts (rest of N3) | Planned |
| Q2 2027 | Mainnet with a real stablecoin, the mainnet MPC signer and NEAR AI signers; a cap on sale size until the audit has run for a quarter | Planned |
| Q2 2027 | A listing page and wallet buy button (today the page only reads the chain) | Planned |

Not planned: custody of anyone's funds outside the agent's own contract; an off-chain order book.
