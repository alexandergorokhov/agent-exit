#!/usr/bin/env node
// Sealed agent config tools (contracts/agent-core/src/sealed.rs, the same
// cipher): X25519 + a SHA-256 keystream. No dependencies beyond Node.
//
//   node scripts/sealed-config.mjs commit <bundle-file>              -> {hash, bytes, root} for commit_config
//   node scripts/sealed-config.mjs proof <bundle-file> <index>       -> {index, chunk (base64), proof} for answer_preview (SC21)
//   node scripts/sealed-config.mjs keygen                            -> a one-time delivery key pair (hex)
//   node scripts/sealed-config.mjs seal <bundle-file> <buyer-pk-hex> -> {ephemeral_pk, ciphertext (base64)} for deliver_config
//   node scripts/sealed-config.mjs open <sk-hex> <ephemeral-pk-hex> <ciphertext-base64> [<hash> [<root>]]
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

// SC21: Merkle tree over 256-byte chunks (the same as sealed.rs chunk_root).
export const CHUNK = 256;
const LEAF = Buffer.from("agent-exit/sealed-chunk/v1");
const u32 = (i) => { const b = Buffer.alloc(4); b.writeUInt32LE(i); return b; };
const leaf = (i, c) => sha(LEAF, u32(i), c);
const node = (l, r) => sha(Buffer.from([1]), l, r);

function levels(bundle) {
  let level = [];
  for (let i = 0; i * CHUNK < bundle.length; i++) level.push(leaf(i, bundle.subarray(i * CHUNK, (i + 1) * CHUNK)));
  if (!level.length) level.push(leaf(0, Buffer.alloc(0)));
  const out = [level];
  while (level.length > 1) {
    const next = [];
    for (let i = 0; i < level.length; i += 2) next.push(i + 1 < level.length ? node(level[i], level[i + 1]) : level[i]);
    level = next;
    out.push(level);
  }
  return out;
}

export const chunkRoot = (bundle) => levels(bundle).at(-1)[0];

/** The chunk at `index` and its sibling path (hex), for answer_preview. */
export function chunkProof(bundle, index) {
  if (!Number.isInteger(index) || index < 0 || index * CHUNK >= Math.max(bundle.length, 1)) throw new Error(`chunk ${index} is out of range`);
  const chunk = bundle.subarray(index * CHUNK, (index + 1) * CHUNK);
  const proof = [];
  let i = index;
  for (const level of levels(bundle)) {
    if (level.length <= 1) break;
    if ((i ^ 1) < level.length) proof.push(level[i ^ 1].toString("hex"));
    i >>= 1;
  }
  return { index, chunk: Buffer.from(chunk).toString("base64"), proof };
}

export function commit(bundle) {
  return { hash: sha(bundle).toString("hex"), bytes: bundle.length, root: chunkRoot(bundle).toString("hex") };
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
  else if (cmd === "proof") console.log(JSON.stringify(chunkProof(readFileSync(a[0]), Number(a[1]))));
  else if (cmd === "keygen") {
    const sk = randomBytes(32);
    console.log(JSON.stringify({ delivery_pk: publicKey(sk).toString("hex"), delivery_sk: sk.toString("hex") }));
  } else if (cmd === "seal") {
    const r = seal(randomBytes(32), hex(a[1]), readFileSync(a[0]));
    console.log(JSON.stringify({ ephemeral_pk: r.ephemeral_pk, ciphertext: r.ciphertext.toString("base64") }));
  } else if (cmd === "open") {
    const plain = open(hex(a[0]), hex(a[1]), Buffer.from(a[2], "base64"));
    const c = commit(plain);
    // Review 5 P1: the contract checks hash AND root; so does this.
    const matches = a[3] ? c.hash === a[3] && (!a[4] || c.root === a[4]) : null;
    console.log(JSON.stringify({ ...c, matches, bundle: plain.toString("utf8") }));
  } else {
    console.error("usage: sealed-config.mjs commit|proof|keygen|seal|open ...");
    process.exit(2);
  }
}
