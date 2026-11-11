// Small pure helpers for the page's money lines, shared with web/test/smoke.mjs.
// All amounts are xUSD-FAKE in cents (integers), so sums are exact.

/** "1,000.00" from 100000 cents. */
export const fmt = (cents) => (cents / 100).toLocaleString("en-US", { minimumFractionDigits: 2, maximumFractionDigits: 2 });

const cents = (s) => Math.round(Number(String(s).replace(/,/g, "")) * 100);

/**
 * The investor payout in the recorded invest run (data/testnet.json, steps.invest):
 * every customer payment after the share buy, the share the investor bought,
 * and what that share comes to. Returns null when the run is not recorded.
 */
export function investorPayout(world) {
  const txs = world?.steps?.invest?.txs || [];
  const share = txs.find((x) => x.ok && x.method === "ft_transfer_call" && /buys the (\d+)% slice/.test(x.label || ""));
  const claim = txs.find((x) => x.ok && x.method === "claim");
  if (!share || !claim) return null;
  const pct = Number(/buys the (\d+)% slice/.exec(share.label)[1]);
  const paid = txs.filter((x) => x.ok && /pays \S+ ([\d,]+\.\d\d)/.test(x.label || ""))
    .map((x) => cents(/pays \S+ ([\d,]+\.\d\d)/.exec(x.label)[1]));
  if (!paid.length) return null;
  const total = paid.reduce((a, b) => a + b, 0);
  return { pct, payments: paid.length, total, investor: Math.round((total * pct) / 100), claim };
}

/** The contract's fee: 1% of every sale and share buy (100 basis points). */
export const FEE_BPS = 100;

/**
 * Where the money of one deal goes, in cents. A sale pays the seller now,
 * less the fee, and holds 30% as earn-out. A share buy pays the seller half
 * now, less the fee, and holds half in escrow for the investor.
 */
export function waterfall(kind, price) {
  const fee = Math.round((price * FEE_BPS) / 10000);
  const held = kind === "sale" ? Math.round((price * 30) / 100) : Math.round(price / 2);
  return { seller: price - held - fee, fee, held, total: price };
}
