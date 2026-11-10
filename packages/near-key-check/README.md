# near-key-check

**Who else can take this NEAR account?** One read-only check for any testnet account: an AI agent's wallet, a trading bot, a vault. It reads the access keys and the code, and says whether anyone other than the contract can move the money or change the code. Zero dependencies; nothing is ever signed or sent.

```bash
node packages/near-key-check/cli.mjs trading-bot.testnet           # exit 0 locked, 1 not locked, 2 error
node packages/near-key-check/cli.mjs my-agent.testnet --published <code_hash> --json
```

| Verdict | Meaning |
|---|---|
| `locked` | no key can change the code or take the account, and the code is the build you trust (`--published`) |
| `no_key_unknown_code` | no full-access key and no upgrade-named method, but the code is not one you can check |
| `backdoor_risk` | code runs here, but a full-access key, an upgrade method, a key the contract never installed, or a global contract another account owns can override it |
| `key_controlled` | an ordinary wallet: whoever holds a full-access key controls it |

## In five minutes: guard your agent before it pays another agent

```js
import { assertNoBackdoor } from "./packages/near-key-check/index.mjs";

// Before paying or buying from another agent: refuse unless it is locked.
await assertNoBackdoor("seller-agent.testnet", { publishedHash: TRUSTED_CODE_HASH });
```

`checkAccount(account, { publishedHash })` returns the verdict, a one-line summary, every finding (`code`, `severity`, `text`) and what each key can do. The rules are the same file as the demo page's "Check any NEAR account" box (`web/account-check.mjs`, checked byte for byte by `test/check.test.mjs`) and give the same verdicts as the MCP tool `check_account`.

Testnet only, by design. Tests: `node --test 'test/*.test.mjs'` (NK1-NK4). MIT.
