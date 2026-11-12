# All testnet transactions

Moved from the README. A scene index (what each scene proves, and its world) is at the end: [Scenes](#scenes).

Recorded on 2026-11-10 in a fresh world under `agentexit5.testnet` (agent code hash `5VqS52NRV3Ak7jiEs95rocWfgaaRV9Z2b8m47R3gCjsF`, the reproducible build in `contracts/res`). Every link below was checked against an archival node (`make check-links`). Money is `xUSD-FAKE`.

| What | Transaction |
|---|---|
| Honest sale: bob buys translator in one `ft_transfer_call` | [oSpSKG9K…](https://testnet.nearblocks.io/txns/oSpSKG9KHLdQXfBDfV2RXDH4EFpp7Toi8PRgB5nZLsC) |
| Old seller withdraws the treasury: `NotOwner` | [FixXD2ty…](https://testnet.nearblocks.io/txns/FixXD2tyGbjKPrjXvL5UsjY3482n8oJWTPCcSD5NRTk6) |
| Old seller adds her own runner key: `NotOwner` | [FZcNjC91…](https://testnet.nearblocks.io/txns/FZcNjC91gsDwhBDi66vTTa11Mh9ccraeFEhDKKCdMtTQ) |
| Old seller changes the spending limits: `NotOwner` | [H5EJSLwN…](https://testnet.nearblocks.io/txns/H5EJSLwN3WmSV2h3pDZJH7urTZo6gBDwoEFHE6YjocoX) |
| Old seller moves the deed to herself: `NotOwner` | [Dj94EMuX…](https://testnet.nearblocks.io/txns/Dj94EMuXnthsHSZenaWXDSf7QDE6EkebaGnppN4eKVRE) |
| Old seller spends from the Base wallet: `NotRunner` | [JA1eZDta…](https://testnet.nearblocks.io/txns/JA1eZDtaaF18extyCQDcHfWMU1HSryKVWbNmLrSYEweb) |
| Old seller's runner key and a key the born-clean agent never had | no transaction possible: the node answers "access key … does not exist" (`out/scenes/scene-1.json`) |
| New owner's runner key works | [BjG6o91U…](https://testnet.nearblocks.io/txns/BjG6o91UwvUcD1pLJBWqB8St9VpeFZ91aiy8vnYRpE13) |
| Spending limit: 8.00 + 45.00 > daily cap 50.00, `PolicyViolation:DailyCap` | [GjNyoNJj…](https://testnet.nearblocks.io/txns/GjNyoNJjf2KLW4dzTiPfrm2iBbowfpCxNNSFEo769y1y) |
| An agent account (summarizer) buys translator; scripted client, no AI model | [Bp48FRJG…](https://testnet.nearblocks.io/txns/Bp48FRJGDYUEMuMDTKRdNVY73YjBcRZfnheK6GibD88q) |
| Gate G1, fresh agent sold and the old keys fail | [BRgPPd4G…](https://testnet.nearblocks.io/txns/BRgPPd4Gv6kfqgRqL593C1PwzHE2y96GsaMqAu4Aa6Jn) |
| Gate G6, a payment verified on-chain with a real NEAR AI Cloud signature (previous world `agentexit4.testnet`) | [GTWFcGyX…](https://testnet.nearblocks.io/txns/GTWFcGyXi3RN6wQL3ZSPcHmP1E1aBHzyUUeaVT4BKudb) |
| Shares (scene 8): dave buys 20% of translator3's earnings | [D4xQARvN…](https://testnet.nearblocks.io/txns/D4xQARvNcaL4euHzvMVWFBt7gg8SnYQGHPuiPbD1ML7B) |
| Shares: a customer pays for a real NEAR AI translation, verified | [CCYk7MR4…](https://testnet.nearblocks.io/txns/CCYk7MR4Tw59iHv7aNcNc2ALtgYcENTm6aM2VCcro4CL) |
| Shares: the owner's sub-account pays, flagged, no escrow released | [HfdsWPaT…](https://testnet.nearblocks.io/txns/HfdsWPaTpgMkQmb3biHCJQAMKd96cWXxiaRSoRbJDk21) |
| Shares: dave claims exactly 20% | [Evd1jGnU…](https://testnet.nearblocks.io/txns/Evd1jGnUhTtdbSJ2uSsf2aSMMgtJLtiN153Uqpk1Fajy) |
| A real AI model (Claude Haiku 4.5, about $0.05) buys an agent through MCP (previous world `agentexit4.testnet`) | [Dh5QgrRb…](https://testnet.nearblocks.io/txns/Dh5QgrRbcij4JcARs1e7YpZVX4wKPtv2ca1RpgTprFXD) |
| Born clean (scene 9): the factory creates `born1` in one batch, no access key ever | [2Rgj3r37…](https://testnet.nearblocks.io/txns/2Rgj3r37wUx22ySBK5PZWM45JNP68U4RxRDHzv5zDpLD) |
| The transaction the page's Invest button builds (faucet, then buy 2.5% of translator3), signed by script for the outside account `p6.agentexit5.testnet` (`scripts/testnet/invest-button.mjs`) | [DHJhBU22…](https://testnet.nearblocks.io/txns/DHJhBU229GZvNWTykNBXZ4134BXjCFA2ixY5SMZBiCSW) |
| An MCP agent invests by a fixed rule: carol buys 2.5% of translator3 through `buy_share` (`make agent-invests-testnet`) | [5ijxcigm…](https://testnet.nearblocks.io/txns/5ijxcigmGFmXDGym3p3S5jf9kwKnPQJ5HineUcoS8aPr) |
| Sealed prompt (scene 10): alice delivers a bundle that is not the committed one | [Hjbfj9ng…](https://testnet.nearblocks.io/txns/Hjbfj9ngPkYJLyk6HtW8gVS7k7534ueaQgHQLhov5D1J) |
| Sealed prompt: bob reveals his one-time key; the contract decrypts the delivery, finds the mismatch and refunds 100.00 | [Bu9CWFGK…](https://testnet.nearblocks.io/txns/Bu9CWFGKo6a61d1VYuwxCaNdD2HYBM3kktfE7KmLahwz) |
| Sealed prompt: alice's late `release_upfront` is refused | [28tRkMRJ…](https://testnet.nearblocks.io/txns/28tRkMRJyd1kwoQGJWoMhnBDqgAFGT3CLjSZYbtxtZRg) |
| A business day (scene 11): the owner signs one `set_limits` (cap 8.00, one payee, split 80/20) | [5rm3FQhE…](https://testnet.nearblocks.io/txns/5rm3FQhEsc6jgmZAVdrLkaD87miHYQ37ekpmqfuHphGE) |
| Business day: the runner key buys a 2.00 supplier job with no human signing | [3Xis64fw…](https://testnet.nearblocks.io/txns/3Xis64fw8dVLKeoeaJ8tXpghCoVVdKCCzomghKSR6f5X) |
| Business day: the fifth job is refused `PolicyViolation:DailyCap` | [ER3it1ht…](https://testnet.nearblocks.io/txns/ER3it1ht6Nn5wppSpCLrNWntnVCtvysgPS389uKW9Rgc) |
| Business day: a payee not on the list is refused `PolicyViolation:Payee` | [6A5Ba8zY…](https://testnet.nearblocks.io/txns/6A5Ba8zYoDN1qdFvELV2145HSsmNzneL144EhNbsN2Jz) |
| Open share offer (scene 12): alice offers 25% of translator3 in 10 parts of 2.5% at 25.00 each; carol's part is the MCP investor row above | recorded state only (`out/scenes/scene-12.json`, `offer`) |
| The Exchange (scene 13): the factory creates translate-pro, born clean (`agent_born` event, no access key) | [8VyRw9PP…](https://testnet.nearblocks.io/txns/8VyRw9PPr5H2bGP4rLzFSzEKJMrDErdoAMJbkExb3Li7) |
| Exchange: research-desk born clean | [52vkFvTK…](https://testnet.nearblocks.io/txns/52vkFvTKg7ymPpfaF9gLpbyDyM2MdPb3pCuP2VR1Tair) |
| Exchange: pump-bot born clean | [QzXQM73j…](https://testnet.nearblocks.io/txns/QzXQM73j19TWocXcYcu6zfCXoX57EVH2GCPaJuVZhQa) |
| Exchange: alice offers 10% of translate-pro's earnings for 50.00 | [D3Ms3YAx…](https://testnet.nearblocks.io/txns/D3Ms3YAxdbXr5CToPdy5PFp4ECGJWQUqFbNbD1KgXqBn) |
| Exchange: alice offers 20% of research-desk's earnings for 100.00 | [6zyAxd72…](https://testnet.nearblocks.io/txns/6zyAxd72KG1tYKPVB7J6JdZ7X1uW1iGLkKiuaDgcecyf) |
| Exchange: alice pays her own pump-bot 100.00 (flagged, not counted) | [6gCvS37P…](https://testnet.nearblocks.io/txns/6gCvS37PgGJ54vpBLz3tDbqbycjchLMz1uLanpkDpPuh) |
| Exchange: alice lists pump-bot at 1,000.00 (410.00 gross, 400.00 flagged, 10.00 counted) | [F4ud23g4…](https://testnet.nearblocks.io/txns/F4ud23g4BGCym936X43Bmyvdhiq3bhAYoka1HhioWo1R) |

Not run on testnet: G2 (the Base Sepolia payment; needs a Base relayer key). The cheat scan (scene 4) and the earnings view (scene 3) are read-only and recorded in `out/scenes/`.

## Scenes

Each `out/scenes/scene-N.json` has `pass`, `what` (what the runner did) and `txs[]` (`tx`, `by`, `receiver`, `method`, `ok`, `error`, `events`, `explorer` = the NearBlocks link). To read one without our server, see [Verify testnet without our server](PROOF.md#verify-testnet-without-our-server).

| Scene | What it proves | World | Key transaction |
|---|---|---|---|
| 1 | Break-in: after the sale every way back in fails; the old keys are gone | agentexit5 | [FixXD2ty…](https://testnet.nearblocks.io/txns/FixXD2tyGbjKPrjXvL5UsjY3482n8oJWTPCcSD5NRTk6) |
| 2 | Spending limits: split 20/5, an 8.00 payment allowed, 45.00 refused `PolicyViolation:DailyCap` | agentexit5 | [GjNyoNJj…](https://testnet.nearblocks.io/txns/GjNyoNJjf2KLW4dzTiPfrm2iBbowfpCxNNSFEo769y1y) |
| 3 | Verified vs unverified earnings, translator and cheat side by side | agentexit5 (view calls only) | none |
| 4 | The cheat is listed; the scan blocks it (`full_access_key_present`); no buy is sent | agentexit5 | [3dZ6MiNs…](https://testnet.nearblocks.io/txns/3dZ6MiNsMSPT1tE3zgfMDj3Qmm13g27kqRuYpLUm5LMi) |
| 5 | Honest sale in one `ft_transfer_call`; keys before and after | agentexit5 | [oSpSKG9K…](https://testnet.nearblocks.io/txns/oSpSKG9KHLdQXfBDfV2RXDH4EFpp7Toi8PRgB5nZLsC) |
| 6 | An agent account buys an agent (scripted client) and checks the lockout | agentexit5 | [Bp48FRJG…](https://testnet.nearblocks.io/txns/Bp48FRJGDYUEMuMDTKRdNVY73YjBcRZfnheK6GibD88q) |
| 7 | Setup for the AI buyer: translator listed at 1,000.00 (the buy is in `out/llm-buyer-testnet.json`) | **agentexit4** | [FyK47wYy…](https://testnet.nearblocks.io/txns/FyK47wYyTJ8xWtS8Yb8hN4xbRugwcRCNBNgTN4i4XYce) |
| 8 | Shares: dave buys 20% of translator3, 5 NEAR AI-verified jobs, a flagged sub-account payment, dave claims exactly 20% | agentexit5 | [D4xQARvN…](https://testnet.nearblocks.io/txns/D4xQARvNcaL4euHzvMVWFBt7gg8SnYQGHPuiPbD1ML7B) |
| 9 | Born clean: the factory creates born1 in one batch, no access key ever | agentexit5 | [2Rgj3r37…](https://testnet.nearblocks.io/txns/2Rgj3r37wUx22ySBK5PZWM45JNP68U4RxRDHzv5zDpLD) |
| 10 | Sealed prompt: a swapped bundle is decrypted on-chain and the buyer gets the whole price back | agentexit5 | [Bu9CWFGK…](https://testnet.nearblocks.io/txns/Bu9CWFGKo6a61d1VYuwxCaNdD2HYBM3kktfE7KmLahwz) |
| 11 | A business day: one owner signature, then no human key; DailyCap and Payee refusals | agentexit5 | [ER3it1ht…](https://testnet.nearblocks.io/txns/ER3it1ht6Nn5wppSpCLrNWntnVCtvysgPS389uKW9Rgc) |
| 12 | An open share offer: 25% of translator3 in parts of 2.5% (state only; carol's buy is in `out/agent-invests-testnet.json`) | agentexit5 | [5ijxcigm…](https://testnet.nearblocks.io/txns/5ijxcigmGFmXDGym3p3S5jf9kwKnPQJ5HineUcoS8aPr) |
| 13 | The Exchange: 3 born-clean agents (translate-pro, research-desk offer shares; pump-bot listed, mostly flagged revenue) | agentexit5 | [F4ud23g4…](https://testnet.nearblocks.io/txns/F4ud23g4BGCym936X43Bmyvdhiq3bhAYoka1HhioWo1R) |
| G1 | A fresh agent sold; the old keys fail (`out/gates/g1.json`) | agentexit5 | [BRgPPd4G…](https://testnet.nearblocks.io/txns/BRgPPd4Gv6kfqgRqL593C1PwzHE2y96GsaMqAu4Aa6Jn) |
| G6 | A real NEAR AI Cloud signature verified by the contract (`out/gates/g6.json`) | **agentexit4** | [GTWFcGyX…](https://testnet.nearblocks.io/txns/GTWFcGyXi3RN6wQL3ZSPcHmP1E1aBHzyUUeaVT4BKudb) |
