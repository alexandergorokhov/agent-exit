// One-click testnet transactions without a wallet SDK: a link to MyNearWallet's
// /sign page carrying a borsh-encoded transaction. MyNearWallet rebuilds the
// transaction from receiverId + actions (fresh nonce and block hash), so the
// nonce, block hash and public key here are placeholders. The signer must be an
// account the visitor has in the wallet. Navigation only: the page's strict CSP
// stays as it is. Pure functions, also used by web/test/smoke.mjs.

export const WALLET_SIGN = "https://testnet.mynearwallet.com/sign";

const enc = new TextEncoder();

function u32(n) { const b = new Uint8Array(4); new DataView(b.buffer).setUint32(0, n, true); return b; }
function u64(n) { const b = new Uint8Array(8); new DataView(b.buffer).setBigUint64(0, BigInt(n), true); return b; }
function u128(n) { const v = BigInt(n); if (v < 0n || v >> 128n) throw new Error("u128 out of range"); return Uint8Array.from([...u64(v & ((1n << 64n) - 1n)), ...u64(v >> 64n)]); }
function bytes(b) { return Uint8Array.from([...u32(b.length), ...b]); }
function str(s) { return bytes(enc.encode(s)); }

const ACCOUNT = /^(([a-z\d]+[-_])*[a-z\d]+\.)*([a-z\d]+[-_])*[a-z\d]+$/;
export function validAccount(id) { return typeof id === "string" && id.length >= 2 && id.length <= 64 && ACCOUNT.test(id); }

// actions: [{ method, args (object), gasTgas (number), deposit (yocto, string|bigint) }]
export function encodeTx({ signerId, receiverId, actions }) {
  if (!validAccount(signerId)) throw new Error(`bad signer ${signerId}`);
  if (!validAccount(receiverId)) throw new Error(`bad receiver ${receiverId}`);
  if (!actions.length) throw new Error("no actions");
  const parts = [str(signerId), new Uint8Array(33) /* ed25519 tag 0 + 32 zero bytes */, u64(0), str(receiverId), new Uint8Array(32), u32(actions.length)];
  for (const a of actions) {
    const gas = BigInt(a.gasTgas) * 10n ** 12n;
    if (gas > 300n * 10n ** 12n) throw new Error("gas over 300 Tgas");
    parts.push(Uint8Array.of(2) /* FunctionCall */, str(a.method), bytes(enc.encode(JSON.stringify(a.args))), u64(gas), u128(a.deposit ?? 0));
  }
  const out = new Uint8Array(parts.reduce((n, p) => n + p.length, 0));
  let i = 0;
  for (const p of parts) { out.set(p, i); i += p.length; }
  return out;
}

export function base64(u8) { let s = ""; for (const b of u8) s += String.fromCharCode(b); return btoa(s); }

export function signUrl(txs, callbackUrl) {
  const u = new URL(WALLET_SIGN);
  u.searchParams.set("transactions", txs.map((t) => base64(encodeTx(t))).join(","));
  if (callbackUrl) u.searchParams.set("callbackUrl", callbackUrl);
  return u.toString();
}

// The page's two buttons, from data/testnet.json's open_offer.
// invest: one transaction to the token: take test dollars from the faucet, then
// buy a 1% part with them (ft_transfer_call carries the offer's exact terms).
export function investTx(offer, signerId) {
  return { signerId, receiverId: offer.xusd, actions: [
    { method: "faucet", args: { amount: offer.amount }, gasTgas: 30, deposit: 0 },
    { method: "ft_transfer_call", args: { receiver_id: offer.agent, amount: offer.amount, msg: offer.msg }, gasTgas: 150, deposit: 1 },
  ] };
}
export function claimTx(offer, signerId) {
  return { signerId, receiverId: offer.agent, actions: [{ method: "claim", args: {}, gasTgas: 50, deposit: 0 }] };
}
