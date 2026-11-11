# All testnet transactions

Moved from the README.

Recorded on 2026-11-10 in a fresh world under `agentexit5.testnet` (agent code hash `5VqS52NRV3Ak7jiEs95rocWfgaaRV9Z2b8m47R3gCjsF`, the reproducible build in `contracts/res`). Every link below was checked against an archival node (`make check-links`). Money is `xUSD-FAKE`.

| What | Transaction |
|---|---|
| Honest sale: bob buys translator in one `ft_transfer_call` | [oSpSKG9K…](https://testnet.nearblocks.io/txns/oSpSKG9KHLdQXfBDfV2RXDH4EFpp7Toi8PRgB5nZLsC) |
| Old seller withdraws the treasury: `NotOwner` | [FixXD2ty…](https://testnet.nearblocks.io/txns/FixXD2tyGbjKPrjXvL5UsjY3482n8oJWTPCcSD5NRTk6) |
| Old seller adds her own runner key: `NotOwner` | [FZcNjC91…](https://testnet.nearblocks.io/txns/FZcNjC91gsDwhBDi66vTTa11Mh9ccraeFEhDKKCdMtTQ) |
| Old seller changes the spending limits: `NotOwner` | [H5EJSLwN…](https://testnet.nearblocks.io/txns/H5EJSLwN3WmSV2h3pDZJH7urTZo6gBDwoEFHE6YjocoX) |
| Old seller moves the deed to herself: `NotOwner` | [Dj94EMuX…](https://testnet.nearblocks.io/txns/Dj94EMuXnthsHSZenaWXDSf7QDE6EkebaGnppN4eKVRE) |
| Old seller spends from the Base wallet: `NotRunner` | [JA1eZDta…](https://testnet.nearblocks.io/txns/JA1eZDtaaF18extyCQDcHfWMU1HSryKVWbNmLrSYEweb) |
| Old seller's runner key and the deleted setup key | no transaction possible: the node answers "access key … does not exist" (`out/scenes/scene-1.json`) |
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
| The page's Invest button transaction (faucet, then buy 2.5% of translator3), signed by script for the outside account `p6.agentexit5.testnet` (`scripts/testnet/invest-button.mjs`) | [DHJhBU22…](https://testnet.nearblocks.io/txns/DHJhBU229GZvNWTykNBXZ4134BXjCFA2ixY5SMZBiCSW) |
| An MCP agent invests by a fixed rule: carol buys 2.5% of translator3 through `buy_share` (`make agent-invests-testnet`) | [5ijxcigm…](https://testnet.nearblocks.io/txns/5ijxcigmGFmXDGym3p3S5jf9kwKnPQJ5HineUcoS8aPr) |

Not run on testnet: G2 (the Base Sepolia payment; needs a Base relayer key). The cheat scan (scene 4) and the earnings view (scene 3) are read-only and recorded in `out/scenes/`.
