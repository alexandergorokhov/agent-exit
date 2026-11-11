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

/**
 * An honest note on an early price ÷ revenue multiple. counted and price are
 * in cents, bps is the share the offer sells, end is a ms timestamp (0: none).
 * Returns null once there is a month of revenue: the multiple then speaks
 * for itself.
 */
export function earlyNote({ days, counted, price, bps, end }) {
  if (!(days > 0) || days >= 30 || !(counted > 0) || !(bps > 0)) return null;
  const value = Math.round((price * 10000) / bps);
  const multiple = `${(value / counted).toFixed(1)}×`;
  const d = `${days} day${days === 1 ? "" : "s"} of revenue`;
  const until = end ? ` until ${new Date(end).toLocaleDateString("en-GB", { day: "numeric", month: "short", year: "numeric", timeZone: "UTC" })}` : "";
  return {
    multiple,
    short: `early: ${d}`,
    text: `Early: ${d} (${fmt(counted)}). The price is ${fmt(price)} for ${bps / 100}% of earnings${until}.`,
  };
}

/**
 * "Investors so far" from the recorded open offer: each holder (bought by the
 * MCP agent that invests by a fixed rule), then the page's own Invest-button
 * transaction, signed by script. Each with a label saying which is which.
 */
export function investorList(offer) {
  const out = (offer?.offer?.holders || []).map((h) => ({ account: h.account, bps: Number(h.bps), label: "MCP agent investor" }));
  const ex = offer?.example_tx;
  if (ex && ex.account && !out.some((h) => h.account === ex.account)) {
    out.push({ account: ex.account, bps: Number(ex.holdings_bps || 0), tx: ex.tx, label: "Invest button tx, signed by script" });
  }
  return out;
}
