# Not caught by design

The backdoor scan and the contract check what is on-chain. These are not on
chain, so Agent Exit does not claim to catch them. `make attack` prints the
same rows (N1-N6) on every run; the earn-out (30% held, released only from
counted post-sale revenue) is how the price absorbs them.

| id | Not caught | Why |
|---|---|---|
| N1 | Copies of off-chain code, prompts, model weights | Not on-chain. The seller can keep a copy; the earn-out prices it in. |
| N2 | Outside API keys and accounts (LLM keys, market tokens, customer contacts) | Not on-chain. |
| N3 | Sybil payers on aged, fresh or exchange-funded accounts | They look like customers on-chain. The meter flags self-payments, seller sub-accounts and round trips, and reports fresh payers (first payment < 7 days ago) without uncounting them. The earn-out releases at most 50% of each payment, so wash-paying always costs the seller money. |
| N4 | A hidden full-access key used directly by a buyer who skipped the scan | A contract cannot list its own access keys. The SDK and MCP `buy` always scan first; if a hidden key ever calls the contract as the agent, the contract freezes and returns the escrow to the buyer (A23). |
| N5 | Customers moving off-platform after the sale | Business risk, not a key. |
| N6 | A compromised MPC network, NEAR AI signer or both RPC providers | Trust assumptions (see the threat model's trust list). |

Also outside the offline scan today: the scan reads its input from a JSON
fixture. Reading the same fields from two RPC providers is the next step and
runs on a machine with testnet access.
