# Use Agent Exit with your own agent

Four ways in, from lightest to deepest. Testnet only; all money is xUSD-FAKE.

| You want | Do | Time |
|---|---|---|
| Know who else can take any NEAR account | `node packages/near-key-check/cli.mjs your-agent.testnet` (or `make key-check ID=...`): `locked`, `no_key_unknown_code`, `backdoor_risk` or `key_controlled`, with every key and what it can do | 1 min |
| Stop your agent paying or buying from a backdoored one | `await assertNoBackdoor(account, { publishedHash })` from [`packages/near-key-check`](../packages/near-key-check/README.md) before each payment | 5 min |
| Let an AI agent (Claude Code, Codex, any MCP client) scan, buy, invest and check accounts | add the MCP server from [`.mcp.json`](../.mcp.json) (see [`AGENTS.md`](../AGENTS.md)); the server, not the model, enforces `confirm`, `max_price`, the spend cap and scan-before-buy | 10 min |
| Make your agent sellable with the seller provably locked out | create it from the born-clean factory: `create_agent(name, owner_id?, base_address?)` with at least 10 NEAR; the account is created, the published code deployed and `new` called in one batch with no key, so no seller key ever existed. Then commit its prompt bundle (`commit_config`), list it (`list_for_sale`) and buyers pay in one `ft_transfer_call` ([`docs/OPERATIONS.md`](OPERATIONS.md)) | 30 min |

The rules behind `near-key-check`, the demo page's account box and the MCP tool `check_account` are one file (`web/account-check.mjs`), so all three give the same verdict on the same account (`mcp/test/account_parity.test.ts`, `packages/near-key-check/test/check.test.mjs`).
