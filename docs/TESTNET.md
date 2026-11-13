# All testnet transactions

Moved from the README. A scene index (what each scene proves, and its world) is at the end: [Scenes](#scenes).

Recorded on 2026-11-13 in a fresh world under `agentexit6.testnet` (agent code hash `3XqAPt3krfpJG9UWFHnZVyDYSws4ZnN4H3B27hCXPuuw`, exactly the reproducible build committed in `contracts/res`). Two rows are marked `agentexit5`: that run was not repeated on the new world. Every link below was checked against an archival node (`make check-links`). Money is `xUSD-FAKE`.

| What | Transaction |
|---|---|
| The cheat (a plain deploy, not born clean): alice's `list_for_sale` at 500.00 is refused `NotBornClean` | [BQrAUGB6…](https://testnet.nearblocks.io/txns/BQrAUGB6yMia6Ep4ZdTSWegoVgZqfP7LJPJV8Dnh99Pu) |
| The cheat: bob sends a buy of 500.00 anyway; the token refunds it in full (`NotBornClean`) | [2Rf95RJf…](https://testnet.nearblocks.io/txns/2Rf95RJft2qHJbtJfXKYGihbnLjtFbpLinuL8wkgQP6H) |
| Honest sale: bob buys translator in one `ft_transfer_call` | [Hh9kTyAP…](https://testnet.nearblocks.io/txns/Hh9kTyAPxL8yMFAawj6rDtf2eGFtGZiXCKuRFnbwBkUs) |
| Old seller withdraws the treasury: `NotOwner` | [5aYWbex9…](https://testnet.nearblocks.io/txns/5aYWbex9PRXeoNQBx7Pny9acwygBn2kmossSuGAukWuw) |
| Old seller adds her own runner key: `NotOwner` | [C9NCHzSN…](https://testnet.nearblocks.io/txns/C9NCHzSNYWoL2cNF1gjc3aCsZgh4kfYnXvMe2NSjnc3b) |
| Old seller changes the spending limits: `NotOwner` | [CV5Jo1D4…](https://testnet.nearblocks.io/txns/CV5Jo1D4GwosqDDh8gFvDvmrXQbzv3UGnQk25gjs5odh) |
| Old seller moves the deed to herself: `NotOwner` | [C52uBdZ4…](https://testnet.nearblocks.io/txns/C52uBdZ4RbWNP5i14SGLaz1BzyguT4B3A1uWvzPQsF1H) |
| Old seller spends from the Base wallet: `NotRunner` | [APU9pV4Z…](https://testnet.nearblocks.io/txns/APU9pV4ZMDVYjTJxKeb1T75Vocncq9bFGSmLdBwQnVjC) |
| Old seller's runner key and a key the born-clean agent never had | no transaction possible: the node answers "access key … does not exist" (`out/scenes/scene-1.json`) |
| New owner's runner key works | [Czy6bKLF…](https://testnet.nearblocks.io/txns/Czy6bKLF1jFmjPciFPQA6RHSatc2b2HbpJPhKEusCTBd) |
| Spending limit: 8.00 + 45.00 > daily cap 50.00, `PolicyViolation:DailyCap` | [FxS4XaWc…](https://testnet.nearblocks.io/txns/FxS4XaWc7FBE4nzR1EH8QossqNf4Q16HKho8FanxiRZM) |
| An agent account (summarizer) buys translator; scripted client, no AI model | [FoEELL43…](https://testnet.nearblocks.io/txns/FoEELL432dz35DJoS7LbXPa8nBAE1cMUykL8GBAP87RF) |
| Gate G1, fresh agent sold and the old keys fail | [38sKE6x9…](https://testnet.nearblocks.io/txns/38sKE6x9u4ggqEgTYuKxEjPPdz7YFM7VhepadoTXPmbn) |
| Gate G6, a payment verified on-chain with a real NEAR AI Cloud signature | [3Z29iLiZ…](https://testnet.nearblocks.io/txns/3Z29iLiZet2ydtqbEy63jM7nx621cosBx1rYYby1b7qY) |
| Shares (scene 8): dave buys 20% of translator3's earnings | [FkU5rFky…](https://testnet.nearblocks.io/txns/FkU5rFkydgtagPRdcGGv4hnivACnEPMku26guehEZhQJ) |
| Shares: a customer pays for a real NEAR AI translation, verified | [A3cvMTc1…](https://testnet.nearblocks.io/txns/A3cvMTc1ZpJkKkLyHVYgeoKWRywJxfNjKTc2EPpFnEEK) |
| Shares: the owner's sub-account pays, flagged, no escrow released | [EvNByMd2…](https://testnet.nearblocks.io/txns/EvNByMd28t71hp6sFMrJvKnSwjc9gU5vNywSn1ki11Mk) |
| Shares: dave claims exactly 20% | [B2L5ft8W…](https://testnet.nearblocks.io/txns/B2L5ft8WL7qorpVD5GaFqshWvaCarxoAypHi56TeYUa7) |
| A real AI model (Claude Haiku 4.5, about $0.05) buys an agent through MCP, after it refuses the cheat | [9zF8jhYZ…](https://testnet.nearblocks.io/txns/9zF8jhYZdocLzjCY5U7NVJtfZnZQ6BSx1YvyFBPP3m8t) |
| Born clean (scene 9): the factory creates `born1` in one batch, no access key ever | [2vrntG2p…](https://testnet.nearblocks.io/txns/2vrntG2pwA9MDbrnEp4WBNvtNuj7hAjR6bDmWECrKo2M) |
| The transaction the page's Invest button builds (faucet, then buy 2.5% of translator3), signed by script for the outside account `p6.agentexit5.testnet` (recorded on the earlier world `agentexit5`) (`scripts/testnet/invest-button.mjs`) | [DHJhBU22…](https://testnet.nearblocks.io/txns/DHJhBU229GZvNWTykNBXZ4134BXjCFA2ixY5SMZBiCSW) |
| An MCP agent invests by a fixed rule: carol buys 2.5% of translator3 through `buy_share` (`make agent-invests-testnet`; recorded on the earlier world `agentexit5`: a per-account daily spend cap refused a re-run on `agentexit6`) | [5ijxcigm…](https://testnet.nearblocks.io/txns/5ijxcigmGFmXDGym3p3S5jf9kwKnPQJ5HineUcoS8aPr) |
| Sealed prompt (scene 10): alice delivers a bundle that is not the committed one | [FbdpqKgx…](https://testnet.nearblocks.io/txns/FbdpqKgx6FHSqYVtYrPBS1kAHR5qMjmj9Xem7oeuTZcP) |
| Sealed prompt: bob reveals his one-time key; the contract decrypts the delivery, finds the mismatch and refunds 100.00 | [8FYLH7MY…](https://testnet.nearblocks.io/txns/8FYLH7MYSh883EgxQdd4mW1jXy5mxPL9t2B8aiN8Gqxr) |
| Sealed prompt: alice's late `release_upfront` is refused | [3AYfbQbK…](https://testnet.nearblocks.io/txns/3AYfbQbKpHM33G2Hd6m337VaPMpQcwp4kPG7ApJw9mqg) |
| A business day (scene 11): the owner signs one `set_limits` (cap 8.00, one payee, split 80/20) | [4V9ofm5p…](https://testnet.nearblocks.io/txns/4V9ofm5pUsPNHLc1PQwzmSRxt9T6Xocc5fN4xTKox3ud) |
| Business day: the runner key buys a 2.00 supplier job with no human signing | [46wtdge8…](https://testnet.nearblocks.io/txns/46wtdge8vH9FQpL5tyyxtYaQBtZ8cSmcspJtfwh5DhkP) |
| Business day: the fifth job is refused `PolicyViolation:DailyCap` | [3NarpCwm…](https://testnet.nearblocks.io/txns/3NarpCwmFZ9G2W5uM3VfEVbp6A45j3rV9FotbViAhRJs) |
| Business day: a payee not on the list is refused `PolicyViolation:Payee` | [2Q68HhaG…](https://testnet.nearblocks.io/txns/2Q68HhaGbYGdon2kRNQbNScjaQEvo3FncPtuN9gzsPcw) |
| Open share offer (scene 12): alice offers 25% of translator3 in 10 parts of 2.5% at 25.00 each; carol's part is the MCP investor row above | recorded state only (`out/scenes/scene-12.json`, `offer`) |
| The Exchange (scene 13): the factory creates translate-pro, born clean (`agent_born` event, no access key) | [7w7Ed7GX…](https://testnet.nearblocks.io/txns/7w7Ed7GX8N5YUdKXHfQktvQoodF7J5JBJhdVzKs4Dk5w) |
| Exchange: alice offers 10% of translate-pro's earnings for 50.00 | [GwpyeGVC…](https://testnet.nearblocks.io/txns/GwpyeGVCeoCyBEyPvB15XGcfeVtLjwZ45PLo3uDGWqhB) |

Not run on testnet: G2 (the Base Sepolia payment; needs a Base relayer key; the signing half is recorded in `out/gates/g2-sign.json` on the earlier world `agentexit5`). The earnings view (scene 3) is read-only and recorded in `out/scenes/`.

## Scenes

Each `out/scenes/scene-N.json` has `pass`, `what` (what the runner did) and `txs[]` (`tx`, `by`, `receiver`, `method`, `ok`, `error`, `events`, `explorer` = the NearBlocks link). To read one without our server, see [Verify testnet without our server](PROOF.md#verify-testnet-without-our-server).

| Scene | What it proves | World | Key transaction |
|---|---|---|---|
| 1 | Break-in: after the sale every way back in fails; the old keys are gone | agentexit6 | [5aYWbex9…](https://testnet.nearblocks.io/txns/5aYWbex9PRXeoNQBx7Pny9acwygBn2kmossSuGAukWuw) |
| 2 | Spending limits: split 20/5, an 8.00 payment allowed, 45.00 refused `PolicyViolation:DailyCap` | agentexit6 | [FxS4XaWc…](https://testnet.nearblocks.io/txns/FxS4XaWc7FBE4nzR1EH8QossqNf4Q16HKho8FanxiRZM) |
| 3 | Verified vs unverified earnings, translator and cheat side by side | agentexit6 (view calls only) | none |
| 4 | The cheat's listing is refused by the contract (`NotBornClean`); a buy sent anyway is refunded in full; the scan also blocks it (`full_access_key_present`) | agentexit6 | [BQrAUGB6…](https://testnet.nearblocks.io/txns/BQrAUGB6yMia6Ep4ZdTSWegoVgZqfP7LJPJV8Dnh99Pu) |
| 5 | Honest sale in one `ft_transfer_call`; keys before and after | agentexit6 | [Hh9kTyAP…](https://testnet.nearblocks.io/txns/Hh9kTyAPxL8yMFAawj6rDtf2eGFtGZiXCKuRFnbwBkUs) |
| 6 | An agent account buys an agent (scripted client) and checks the lockout | agentexit6 | [FoEELL43…](https://testnet.nearblocks.io/txns/FoEELL432dz35DJoS7LbXPa8nBAE1cMUykL8GBAP87RF) |
| 7 | Setup for the AI buyer: translator listed at 1,000.00 (the buy is in `out/llm-buyer-testnet.json`) | agentexit6 | [3ouuHjHK…](https://testnet.nearblocks.io/txns/3ouuHjHKqME99r3xJXcWRXw51crdUcizuEVuPd6JL92J) |
| 8 | Shares: dave buys 20% of translator3, 5 NEAR AI-verified jobs, a flagged sub-account payment, dave claims exactly 20% | agentexit6 | [FkU5rFky…](https://testnet.nearblocks.io/txns/FkU5rFkydgtagPRdcGGv4hnivACnEPMku26guehEZhQJ) |
| 9 | Born clean: the factory creates born1 in one batch, no access key ever | agentexit6 | [2vrntG2p…](https://testnet.nearblocks.io/txns/2vrntG2pwA9MDbrnEp4WBNvtNuj7hAjR6bDmWECrKo2M) |
| 10 | Sealed prompt: a swapped bundle is decrypted on-chain and the buyer gets the whole price back | agentexit6 | [8FYLH7MY…](https://testnet.nearblocks.io/txns/8FYLH7MYSh883EgxQdd4mW1jXy5mxPL9t2B8aiN8Gqxr) |
| 11 | A business day: one owner signature, then no human key; DailyCap and Payee refusals | agentexit6 | [3NarpCwm…](https://testnet.nearblocks.io/txns/3NarpCwmFZ9G2W5uM3VfEVbp6A45j3rV9FotbViAhRJs) |
| 12 | An open share offer: 25% of translator3 in parts of 2.5% at 25.00 (state only; carol's buy is in `out/agent-invests-testnet.json`, recorded on `agentexit5`) | agentexit6 | [F4JJEwJX…](https://testnet.nearblocks.io/txns/F4JJEwJXswpuF2EGgpSECTDk9xx1yoSzy3SE6JfGqyQb) |
| 13 | The Exchange: translate-pro born clean, 5 payers, an open offer of 10% | agentexit6 | [GwpyeGVC…](https://testnet.nearblocks.io/txns/GwpyeGVCeoCyBEyPvB15XGcfeVtLjwZ45PLo3uDGWqhB) |
| G1 | A fresh agent sold; the old keys fail (`out/gates/g1.json`) | agentexit6 | [38sKE6x9…](https://testnet.nearblocks.io/txns/38sKE6x9u4ggqEgTYuKxEjPPdz7YFM7VhepadoTXPmbn) |
| G6 | A real NEAR AI Cloud signature verified by the contract (`out/gates/g6.json`) | agentexit6 | [3Z29iLiZ…](https://testnet.nearblocks.io/txns/3Z29iLiZet2ydtqbEy63jM7nx621cosBx1rYYby1b7qY) |
