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
| N4 | A hidden full-access key used directly by a buyer who skipped the scan | A contract cannot list its own access keys, and a full-access key can redeploy the code or delete keys without any contract code running. So the defence is the scan, before money moves: the SDK and MCP `buy` always scan first. A second, weaker line: if a hidden key ever calls the contract as the agent, the contract freezes and returns the escrow to the buyer (A23). |
| N5 | Customers moving off-platform after the sale | Business risk, not a key. |
| N6 | A compromised MPC network, NEAR AI signer or both RPC providers | Trust assumptions (see the threat model's trust list). |
| N7 | "Verified" payments from the seller's own customers-of-convenience | A NEAR AI signature proves an inference ran for that payer, job and amount, not that the payer is independent. A seller can run its own worker for Sybil payers (N3 still applies). The signer list is set by the registry admin and is not checked by the scan. |
| N8 | Shares: an owner routing customers to an account with no meter | No contract can see off-chain routing. The price escrow limits the loss to the half paid upfront, and the meter shows the drop. |

The scan's input comes from a JSON fixture offline; on testnet the MCP
server reads the same fields from two RPC providers and refuses when they
disagree. Not read through MCP yet: open Base authorizations (`open_auth_until`)
and sub-accounts; the contract still refuses a buy while a Base authorization
is open.
