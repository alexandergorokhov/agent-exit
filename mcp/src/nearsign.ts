// Minimal NEAR transaction signing for the testnet buy: one FunctionCall
// action, ed25519 keys, borsh by hand, signed with node:crypto. No SDK
// dependency, so the whole signing path is these ~120 lines. Cross-checked
// against near-api-js 5.1.1 byte for byte (test NS1).
import { createHash, createPrivateKey, createPublicKey, sign as edSign } from "node:crypto";

const ALPHABET = "123456789ABCDEFGHJKLMNPQRSTUVWXYZabcdefghijkmnopqrstuvwxyz";

export function b58decode(s: string): Buffer {
  let n = 0n;
  for (const c of s) {
    const i = ALPHABET.indexOf(c);
    if (i < 0) throw new Error("bad base58");
    n = n * 58n + BigInt(i);
  }
  const bytes: number[] = [];
  while (n > 0n) {
    bytes.unshift(Number(n & 0xffn));
    n >>= 8n;
  }
  for (const c of s) {
    if (c !== "1") break;
    bytes.unshift(0);
  }
  return Buffer.from(bytes);
}

export function b58encode(b: Uint8Array): string {
  let n = 0n;
  for (const x of b) n = (n << 8n) | BigInt(x);
  let s = "";
  while (n > 0n) {
    s = ALPHABET[Number(n % 58n)] + s;
    n /= 58n;
  }
  for (const x of b) {
    if (x !== 0) break;
    s = "1" + s;
  }
  return s;
}

/** An ed25519 key pair from NEAR's "ed25519:<base58 of 64 bytes>" secret. */
export class NearKey {
  readonly seed: Buffer;
  readonly publicKey: Buffer;
  constructor(secret: string) {
    const m = /^ed25519:([1-9A-HJ-NP-Za-km-z]+)$/.exec(secret.trim());
    if (!m) throw new Error("not an ed25519 secret key (ed25519:<base58>)");
    const raw = b58decode(m[1]);
    if (raw.length !== 64 && raw.length !== 32) throw new Error("ed25519 secret must be 32 or 64 bytes");
    this.seed = raw.subarray(0, 32);
    const der = Buffer.concat([Buffer.from("302e020100300506032b657004220420", "hex"), this.seed]);
    const pub = createPublicKey(createPrivateKey({ key: der, format: "der", type: "pkcs8" })).export({ format: "der", type: "spki" });
    this.publicKey = Buffer.from(pub.subarray(pub.length - 32));
    if (raw.length === 64 && !raw.subarray(32).equals(this.publicKey)) throw new Error("secret key halves do not match");
  }
  get publicKeyString(): string {
    return `ed25519:${b58encode(this.publicKey)}`;
  }
  sign(msg: Buffer): Buffer {
    const der = Buffer.concat([Buffer.from("302e020100300506032b657004220420", "hex"), this.seed]);
    return edSign(null, msg, createPrivateKey({ key: der, format: "der", type: "pkcs8" }));
  }
}

class Borsh {
  parts: Buffer[] = [];
  u8(v: number) {
    this.parts.push(Buffer.from([v]));
  }
  u32(v: number) {
    const b = Buffer.alloc(4);
    b.writeUInt32LE(v);
    this.parts.push(b);
  }
  u64(v: bigint) {
    const b = Buffer.alloc(8);
    b.writeBigUInt64LE(v);
    this.parts.push(b);
  }
  u128(v: bigint) {
    const b = Buffer.alloc(16);
    b.writeBigUInt64LE(v & 0xffffffffffffffffn);
    b.writeBigUInt64LE(v >> 64n, 8);
    this.parts.push(b);
  }
  bytes(b: Uint8Array) {
    this.u32(b.length);
    this.parts.push(Buffer.from(b));
  }
  fixed(b: Uint8Array) {
    this.parts.push(Buffer.from(b));
  }
  str(s: string) {
    this.bytes(Buffer.from(s, "utf8"));
  }
  done(): Buffer {
    return Buffer.concat(this.parts);
  }
}

export interface FunctionCallTx {
  signerId: string;
  publicKey: Buffer;
  nonce: bigint;
  receiverId: string;
  blockHash: Buffer;
  methodName: string;
  args: Buffer;
  gas: bigint;
  deposit: bigint;
}

/** borsh(Transaction V0) with one FunctionCall action (enum index 2). */
export function serializeTx(t: FunctionCallTx): Buffer {
  const w = new Borsh();
  w.str(t.signerId);
  w.u8(0);
  w.fixed(t.publicKey);
  w.u64(t.nonce);
  w.str(t.receiverId);
  if (t.blockHash.length !== 32) throw new Error("block hash must be 32 bytes");
  w.fixed(t.blockHash);
  w.u32(1);
  w.u8(2);
  w.str(t.methodName);
  w.bytes(t.args);
  w.u64(t.gas);
  w.u128(t.deposit);
  return w.done();
}

/** borsh(SignedTransaction): the transaction, then the ed25519 signature of sha256(tx). */
export function signTx(t: FunctionCallTx, key: NearKey): { signed: Buffer; hash: string } {
  const tx = serializeTx(t);
  const digest = createHash("sha256").update(tx).digest();
  const sig = key.sign(digest);
  return { signed: Buffer.concat([tx, Buffer.from([0]), sig]), hash: b58encode(digest) };
}
