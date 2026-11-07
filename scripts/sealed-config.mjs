#!/usr/bin/env node
// Sealed agent config tools (contracts/agent-core/src/sealed.rs, the same
// cipher): X25519 + a SHA-256 keystream. No dependencies beyond Node.
//
//   node scripts/sealed-config.mjs commit <bundle-file>              -> {hash, bytes} for commit_config
//   node scripts/sealed-config.mjs keygen                            -> a one-time delivery key pair (hex)
//   node scripts/sealed-config.mjs seal <bundle-file> <buyer-pk-hex> -> {ephemeral_pk, ciphertext (base64)} for deliver_config
//   node scripts/sealed-config.mjs open <sk-hex> <ephemeral-pk-hex> <ciphertext-base64> [<hash>]
//                                                                    -> the bundle, and whether it matches the commitment
// Secret keys are printed only by keygen; keep them in a 0600 file, never in a chat or commit.
import { createHash, createPrivateKey, createPublicKey, diffieHellman, randomBytes } from "node:crypto";
import { readFileSync } from "node:fs";

const DOMAIN = Buffer.from("agent-exit/sealed-config/v1");
const PKCS8 = Buffer.from("302e020100300506032b656e04220420", "hex");
const SPKI = Buffer.from("302a300506032b656e032100", "hex");
const sha = (...b) => createHash("sha256").update(Buffer.concat(b)).digest();

export const priv = (sk) => createPrivateKey({ key: Buffer.concat([PKCS8, sk]), format: "der", type: "pkcs8" });
export const pub = (pk) => createPublicKey({ key: Buffer.concat([SPKI, pk]), format: "der", type: "spki" });
export const publicKey = (sk) => createPublicKey(priv(sk)).export({ format: "der", type: "spki" }).subarray(SPKI.length);

function stream(key, data) {
  const out = Buffer.alloc(data.length);
  for (let i = 0; i * 32 < data.length; i++) {
    const ctr = Buffer.alloc(4);
    ctr.writeUInt32LE(i);
    const block = sha(key, ctr);
    for (let j = 0; j < 32 && i * 32 + j < data.length; j++) out[i * 32 + j] = data[i * 32 + j] ^ block[j];
  }
  return out;
}

function shared(sk, pk) {
  const s = diffieHellman({ privateKey: priv(sk), publicKey: pub(pk) });
  if (s.every((b) => b === 0)) throw new Error("low-order key");
  return s;
}

export function commit(bundle) {
  return { hash: sha(bundle).toString("hex"), bytes: bundle.length };
}

export function seal(ephSk, recipientPk, bundle) {
  const ephPk = publicKey(ephSk);
  const key = sha(DOMAIN, shared(ephSk, recipientPk), ephPk, recipientPk);
  return { ephemeral_pk: ephPk.toString("hex"), ciphertext: stream(key, bundle) };
}

export function open(sk, ephPk, cipher) {
  const key = sha(DOMAIN, shared(sk, ephPk), ephPk, publicKey(sk));
  return stream(key, cipher);
}

if (import.meta.url === `file://${process.argv[1]}`) {
  const [cmd, ...a] = process.argv.slice(2);
  const hex = (s) => Buffer.from(s, "hex");
  if (cmd === "commit") console.log(JSON.stringify(commit(readFileSync(a[0]))));
  else if (cmd === "keygen") {
    const sk = randomBytes(32);
    console.log(JSON.stringify({ delivery_pk: publicKey(sk).toString("hex"), delivery_sk: sk.toString("hex") }));
  } else if (cmd === "seal") {
    const r = seal(randomBytes(32), hex(a[1]), readFileSync(a[0]));
    console.log(JSON.stringify({ ephemeral_pk: r.ephemeral_pk, ciphertext: r.ciphertext.toString("base64") }));
  } else if (cmd === "open") {
    const plain = open(hex(a[0]), hex(a[1]), Buffer.from(a[2], "base64"));
    const c = commit(plain);
    console.log(JSON.stringify({ ...c, matches: a[3] ? c.hash === a[3] : null, bundle: plain.toString("utf8") }));
  } else {
    console.error("usage: sealed-config.mjs commit|keygen|seal|open ...");
    process.exit(2);
  }
}
