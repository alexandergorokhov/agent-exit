# All testnet transactions

Moved from the README.

Recorded on 2026-11-10 in a fresh world under `agentexit4.testnet` (agent code hash `6wb4xz3ftpzZ2BKu7PjmxMw6Nc7eq8CdanY5hFncr9T1`, the reproducible build in `contracts/res`). Every link below was checked against an archival node (`make check-links`). Money is `xUSD-FAKE`.

| What | Transaction |
|---|---|
| Honest sale: bob buys translator in one `ft_transfer_call` | [5zmqifNE…](https://testnet.nearblocks.io/txns/5zmqifNENpELRixRL6FPqdDfBjTHv7kVGtkmYQpxDZw1) |
| Old seller withdraws the treasury: `NotOwner` | [2B1JuT28…](https://testnet.nearblocks.io/txns/2B1JuT28qv9XHxjcsSMGjpNc24TuK1uxD3LNh6F438mw) |
| Old seller adds her own runner key: `NotOwner` | [3MUkSn5X…](https://testnet.nearblocks.io/txns/3MUkSn5X1sM5M7BnXych9ScS9CtdTTSu1v954qD6ETas) |
| Old seller changes the spending limits: `NotOwner` | [F9n6sumv…](https://testnet.nearblocks.io/txns/F9n6sumvpzDJ1zV7sZrXQT2ZQHuSKvimaKEMLr75JaNw) |
| Old seller moves the deed to herself: `NotOwner` | [8me4WSvb…](https://testnet.nearblocks.io/txns/8me4WSvbniox842HzN9qr7ovt2zDAzSoeskk79LCXSV1) |
| Old seller spends from the Base wallet: `NotRunner` | [72MskCjA…](https://testnet.nearblocks.io/txns/72MskCjArnruKkkW2XqvWhu6r15c5qHwx8knoF7LNACb) |
| Old seller's runner key and the deleted setup key | no transaction possible: the node answers "access key … does not exist" (`out/scenes/scene-1.json`) |
| New owner's runner key works | [7JrQm3aU…](https://testnet.nearblocks.io/txns/7JrQm3aUMv5Raxkz3pEzccGtjFbHYNvAwZD72SyczFrm) |
| Spending limit: 8.00 + 45.00 > daily cap 50.00, `PolicyViolation:DailyCap` | [Hamujmmv…](https://testnet.nearblocks.io/txns/HamujmmvRqDudFaE7WPFw2WKHTa7ewBs4RTDgQziU8jx) |
| An agent account (summarizer) buys translator; scripted client, no AI model | [FrVG2TkY…](https://testnet.nearblocks.io/txns/FrVG2TkYe8gdV9Fq6rfwxhteh3bEt2SR5MaAfhaxojgH) |
| Gate G1, fresh agent sold and the old keys fail | [5jFyMse9…](https://testnet.nearblocks.io/txns/5jFyMse9R5rfV77y3Af9eGyMArjacXi7fUWoduqoeYiP) |
| Gate G6, a payment verified on-chain with a real NEAR AI Cloud signature | [GTWFcGyX…](https://testnet.nearblocks.io/txns/GTWFcGyXi3RN6wQL3ZSPcHmP1E1aBHzyUUeaVT4BKudb) |
| Shares (scene 8): dave buys 20% of translator3's earnings | [HAmKDLz9…](https://testnet.nearblocks.io/txns/HAmKDLz9vK5jM7FefsQA8ASTjwxLeyr4W6U4NgdLUKdw) |
| Shares: a customer pays for a real NEAR AI translation, verified | [AHvnkfEW…](https://testnet.nearblocks.io/txns/AHvnkfEWoE2Pw7Wp7nQWHJPvXRFWzyB1dDp5DgdX17n9) |
| Shares: the owner's sub-account pays, flagged, no escrow released | [xbHWtktX…](https://testnet.nearblocks.io/txns/xbHWtktXZzX2vJ5MgRGhWtGRcxmrLTMLMgEtH6TSGeF) |
| Shares: dave claims exactly 20% | [8BTsC3BE…](https://testnet.nearblocks.io/txns/8BTsC3BEVoP8qvkcyT61yc6uQfKC99kBZCAhXQhKn4DB) |
| A real AI model (Claude Haiku) buys an agent through MCP (Claude Haiku 4.5, about $0.05) | [Dh5QgrRb…](https://testnet.nearblocks.io/txns/Dh5QgrRbcij4JcARs1e7YpZVX4wKPtv2ca1RpgTprFXD) |

Not run on testnet: G2 (the Base Sepolia payment; needs a Base relayer key). The cheat scan (scene 4) and the earnings view (scene 3) are read-only and recorded in `out/scenes/`.
