# All testnet transactions

Moved from the README.

Recorded on 2026-11-04 in a fresh world under `w2.agentexit3.testnet` (agent code hash `Aots3gb33Yjm2pnhgdMYaMdDDk4ZRDnTgsaUA66nDDXq`, the reproducible build in `contracts/res`). Every link below was checked against an archival node (`make check-links`). Money is `xUSD-FAKE`.

| What | Transaction |
|---|---|
| Honest sale: bob buys translator in one `ft_transfer_call` | [B5MfL9V3…](https://testnet.nearblocks.io/txns/B5MfL9V3WBr1uMKs4HPt5JCbw1cyRzQ7nj15S3PDE3Xp) |
| Old seller withdraws the treasury: `NotOwner` | [6peX6VpZ…](https://testnet.nearblocks.io/txns/6peX6VpZmArYjN61RNFjMb25QXijcPrT1mrJMmXnJREE) |
| Old seller adds her own runner key: `NotOwner` | [GwJXKMmV…](https://testnet.nearblocks.io/txns/GwJXKMmVDArhfsPVsRFiyWnYq4n5xbodAypcyCvr3bek) |
| Old seller changes the spending limits: `NotOwner` | [4A16wgc5…](https://testnet.nearblocks.io/txns/4A16wgc5xGaRrD9TDfgnLX2m9n7V5sZV3xSnTfy4E1bx) |
| Old seller moves the deed to herself: `NotOwner` | [2e8b4hjL…](https://testnet.nearblocks.io/txns/2e8b4hjL7hhrC48X2Hs21FgGqXrTnzFNRQrtyuJEgqbX) |
| Old seller spends from the Base wallet: `NotRunner` | [7E5tq2i5…](https://testnet.nearblocks.io/txns/7E5tq2i5kwEYEY9QYCkR9LPBQDDRGUR2wuRgQMcBiWxq) |
| Old seller's runner key and the deleted setup key | no transaction possible: the node answers "access key … does not exist" (`out/scenes/scene-1.json`) |
| New owner's runner key works | [2kH4Lsdw…](https://testnet.nearblocks.io/txns/2kH4Lsdw5TnnFL4QFx8BtUWRdagoizYH7ZUfU6G7dXkK) |
| Spending limit: 8.00 + 45.00 > daily cap 50.00, `PolicyViolation:DailyCap` | [4w9Q9Lh1…](https://testnet.nearblocks.io/txns/4w9Q9Lh1JGrHFDQ8o4D2FqXKZhkxUG6XkxeoLoQMnE6P) |
| An agent account (summarizer) buys translator; scripted client, no AI model | [5EAhSAb2…](https://testnet.nearblocks.io/txns/5EAhSAb236hq36J5Ga3cpvvcnQJbspFeE2XzCSu1Nwdn) |
| Gate G1, fresh agent sold and the old keys fail | [8SXLozby…](https://testnet.nearblocks.io/txns/8SXLozbywPwphxZ6M9T77YmhUYyFk9Ha3Xfy7QhB6Lo4) |
| Gate G6, a payment verified on-chain with a real NEAR AI Cloud signature | [DVjpb5it…](https://testnet.nearblocks.io/txns/DVjpb5itVu2aZ2NQJo2phwvb9K5cjvMMUdEvsDcmYGs8) |
| Shares (scene 8): dave buys 20% of translator3's earnings | [CVHZAS8R…](https://testnet.nearblocks.io/txns/CVHZAS8RYZG5cpnKiHV4RhqmvvBPrtCA41TBLQ42LLCD) |
| Shares: a customer pays for a real NEAR AI translation, verified | [6Kcfgniw…](https://testnet.nearblocks.io/txns/6KcfgniwKPWJkAUju8ZNvxooDv1WUzPqhube5W4BtZcu) |
| Shares: the owner's sub-account pays, flagged, no escrow released | [E8tDPSq7…](https://testnet.nearblocks.io/txns/E8tDPSq7rZc5XHnGpioRTbnrJDxJd7FGiSeBk9ZB22X6) |
| Shares: dave claims exactly 20% | [hphLreQM…](https://testnet.nearblocks.io/txns/hphLreQMrU3pFxS4rhFLiYVHXG2Fe4jV6Ycm36rZqKJ) |
| A real AI model (Claude Haiku) buys an agent through MCP (run on `agentexit3.testnet`) | [6PjsAoL5…](https://testnet.nearblocks.io/txns/6PjsAoL5UZk5WyXrp9Xs1jzesAfYWbiSksMC3Kq2cdwn) |

Not run on testnet: G2 (the Base Sepolia payment; needs a Base relayer key). The cheat scan (scene 4) and the earnings view (scene 3) are read-only and recorded in `out/scenes/`.
