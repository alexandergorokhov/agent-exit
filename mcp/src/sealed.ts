// The sealed-config cipher (contracts/agent-core/src/sealed.rs; the same as
// scripts/sealed-config.mjs): X25519, then a SHA-256 keystream. Used by the
// sealed_delivery tool to open a delivery locally before accepting or
// disputing it, so the buyer never disputes a matching bundle.
import { createHash, createPrivateKey, createPublicKey, diffieHellman } from "node:crypto";

const DOMAIN = Buffer.from("agent-exit/sealed-config/v1");
const PKCS8 = Buffer.from("302e020100300506032b656e04220420", "hex");
const SPKI = Buffer.from("302a300506032b656e032100", "hex");
const sha = (...b: Buffer[]): Buffer => createHash("sha256").update(Buffer.concat(b)).digest();
const priv = (sk: Buffer) => createPrivateKey({ key: Buffer.concat([PKCS8, sk]), format: "der", type: "pkcs8" });
const pub = (pk: Buffer) => createPublicKey({ key: Buffer.concat([SPKI, pk]), format: "der", type: "spki" });

export const publicKey = (sk: Buffer): Buffer => createPublicKey(priv(sk)).export({ format: "der", type: "spki" }).subarray(SPKI.length);

function stream(key: Buffer, data: Buffer): Buffer {
  const out = Buffer.alloc(data.length);
  for (let i = 0; i * 32 < data.length; i++) {
    const ctr = Buffer.alloc(4);
    ctr.writeUInt32LE(i);
    const block = sha(key, ctr);
    for (let j = 0; j < 32 && i * 32 + j < data.length; j++) out[i * 32 + j] = data[i * 32 + j] ^ block[j];
  }
  return out;
}

function shared(sk: Buffer, pk: Buffer): Buffer {
  const s = diffieHellman({ privateKey: priv(sk), publicKey: pub(pk) });
  if (s.every((b) => b === 0)) throw new Error("low-order key");
  return s;
}

export function commitOf(bundle: Buffer): { hash: string; bytes: number } {
  return { hash: sha(bundle).toString("hex"), bytes: bundle.length };
}

export function seal(ephSk: Buffer, recipientPk: Buffer, bundle: Buffer): { ephemeral_pk: string; ciphertext: Buffer } {
  const ephPk = publicKey(ephSk);
  return { ephemeral_pk: ephPk.toString("hex"), ciphertext: stream(sha(DOMAIN, shared(ephSk, recipientPk), ephPk, recipientPk), bundle) };
}

export function open(sk: Buffer, ephPk: Buffer, cipher: Buffer): Buffer {
  return stream(sha(DOMAIN, shared(sk, ephPk), ephPk, publicKey(sk)), cipher);
}
