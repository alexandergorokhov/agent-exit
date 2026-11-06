# Not caught by design

The backdoor scan and the contract check what is on-chain. These are not on
chain, so Agent Exit does not claim to catch them. `make attack` prints the
rows N1-N6 on every run (N7-N11 below cover Verified payments, shares and one MCP testnet gap); the earn-out (30% held, released only from
counted post-sale revenue) is how the price absorbs them.

| id | Not caught | Why |
|---|---|---|
| N1 | Copies of off-chain code, prompts, model weights | Not on-chain. The seller can keep a copy; the earn-out prices it in. |
| N2 | Outside API keys and accounts (LLM keys, market tokens, customer contacts) | Not on-chain. |
| N3 | Sybil payers on aged, fresh or exchange-funded accounts | They look like customers on-chain. The meter flags self-payments, seller sub-accounts and round trips, and reports fresh payers (first payment < 7 days ago) without uncounting them. After the sale the earn-out releases at most 50% of each payment, so wash-paying then costs the seller money. **Before the sale it costs almost nothing** (review 5, H1): the owner can move funds through two hops, or use the test token's faucet, and pay as a stranger; the meter counts it. The scan reports fresh payers but does not block them. The buyer's due-diligence card and MCP `get_earnings` (`revenue_quality`) now weigh it: revenue from payers first seen in the last 7 days counts half, and few payers or under 7 days of history are flagged. Sybils on older accounts still pass; rely on Verified payments and the earn-out. |
| N4 | A hidden full-access key used directly by a buyer who skipped the scan | A contract cannot list its own access keys, and a full-access key can redeploy the code or delete keys without any contract code running. So the defence is the scan, before money moves: the SDK and MCP `buy` always scan first. A second, weaker line: if a hidden key ever calls the contract as the agent, the contract freezes and returns the escrow to the buyer (A23). |
| N5 | Customers moving off-platform after the sale | Business risk, not a key. |
| N6 | A compromised MPC network, NEAR AI signer or both RPC providers | Trust assumptions (see the threat model's trust list). |
| N7 | "Verified" payments from the seller's own customers-of-convenience | A NEAR AI signature proves an inference ran for that payer, job and amount, not that the payer is independent. A seller can run its own worker for Sybil payers (N3 still applies). The signer list is set by the registry admin and is not checked by the scan. |
| N8 | Shares: an owner routing customers to an account with no meter | No contract can see off-chain routing. The price escrow limits the loss to the half paid upfront, and the meter shows the drop. |
| N9 | Shares: unflagged sock-puppet payments | A payment from an account the meter can't tie to the owner releases the investor's escrow like a real customer's. The investor still gets the same money, but a dead agent can look like it pays. Only Verified payments prove an inference ran. |
| N10 | Shares plus earn-out: a seller's friend holding up to 49% | With the earn-out releasing up to 50% of each payment, wash-paying after a sale costs the seller as little as 1% of what is washed. The buyer is not out of pocket (they keep the rest as revenue), but post-sale revenue can look real cheaply. A buyer should read `shares_disclosed`; the scan blocks a listing above 25% (`shares_over_max`, C31) and the MCP server refuses the same. |
| N11 | Sub-accounts on testnet through MCP | NEAR RPC cannot list an account's sub-accounts, so the scan's warn-only `subaccount_does_not_move` rule fires only on offline fixtures. Payments from the seller's sub-accounts are still flagged by the contract (it checks the payer's name). An indexer could fill this in. |
| N12 | State written before the published code was deployed (review 5, C1) | NEAR keeps an account's storage when new code is deployed, so a seller could write state with a throw-away contract and then deploy the published wasm on top; the code hash then matches. The scan now checks that what the contract says it holds is backed by its token balance (from both providers) and that the contract's own ledger check passes (`held_exceeds_balance`, `ledger_insolvent`, scenario C21), so forged claims cannot take the buyer's escrow. Forged earnings history is not caught (it is N3 by other means). Fully closing this needs the account's code-change history from an archival node or indexer. |
| N13 | Admin-side griefing (review 5, M1) | The registry admin could add NEAR AI signers without limit, which grows the state the buy must load. The admin cannot move funds, keys or the deed. The contract now caps the list at 8 (`MAX_SIGNERS`, C27); live once the next testnet deploy runs. |

The scan's input comes from a JSON fixture offline; on testnet the MCP
server reads the same fields from two RPC providers and refuses when they
disagree. Not read through MCP yet: open Base authorizations (`open_auth_until`)
and sub-accounts; the contract still refuses a buy while a Base authorization
is open.
