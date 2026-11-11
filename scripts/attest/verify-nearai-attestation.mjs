#!/usr/bin/env node
// G7: is the NEAR AI Cloud key that signs "verified" payments really inside a
// TEE? Checks NEAR AI Cloud's public attestation report for the model used in
// gate G6 against the signer the contract trusts.
//
//   node scripts/attest/verify-nearai-attestation.mjs fetch    # online: new nonce, fetch, check, save
//   node scripts/attest/verify-nearai-attestation.mjs check    # online re-check of the saved report
//   node scripts/attest/verify-nearai-attestation.mjs check --offline
//
// Each check is pass, fail or unverified, with a reason. Local checks (no network):
//   address_is_contract_signer   the report's signing address = the signer in out/gates/g6.json
//   public_key_derives_address   keccak256(signing_public_key)[12..] = signing address
//   nonce_echoed                 the report answers our nonce (not a replay)
//   tdx_report_data_binds_address_and_nonce  TDX quote report_data = address || 12 zero bytes || nonce
//   gpu_evidence_carries_nonce   the NVIDIA evidence was produced for our nonce
// Network checks (unverified offline):
//   tdx_quote_verifies           Intel DCAP verification of the quote by Phala's public verifier
//   gpu_attestation_nras         NVIDIA NRAS result, its JWT signature checked against NVIDIA's JWKS
//   address_in_testnet_signer_list  the contract on testnet lists the address in get_status.signers
// Which code (G7b, local): the quote's MRTD/RTMR0-3 equal the report's; sha256 of
// the report's app_compose is compose_hash and the quote's MRCONFIGID binds it; the
// RTMR3 event log replays to the quote (digests recomputed from payloads); the
// os_image_hash is a published dstack release; the compose's images are pinned.
// Not checked: MRTD/RTMR0-2 against a reference (none is published; it takes
// building the dstack image), image provenance, and the model-serving containers
// the compose-manager launches (not in the measured compose). The report costs
// nothing: no completion is made.
import { createHash, createPublicKey, randomBytes, verify as edVerify } from "node:crypto";
import { readFileSync, writeFileSync } from "node:fs";
import { fileURLToPath } from "node:url";

const BASE = "https://cloud-api.near.ai/v1";
const PHALA = "https://cloud-api.phala.network/api/v1/attestations/verify";
const NRAS = "https://nras.attestation.nvidia.com/v3/attest/gpu";
const NRAS_JWKS = "https://nras.attestation.nvidia.com/.well-known/jwks.json";
const RPC = "https://rpc.testnet.fastnear.com";
const OUT = "out/gates/g7-attestation.json";

// ------------------------------------------------------------------ keccak-256 (Ethereum's, pad 0x01)
const RC = [0x1n, 0x8082n, 0x800000000000808an, 0x8000000080008000n, 0x808bn, 0x80000001n, 0x8000000080008081n, 0x8000000000008009n, 0x8an, 0x88n, 0x80008009n, 0x8000000an, 0x8000808bn, 0x800000000000008bn, 0x8000000000008089n, 0x8000000000008003n, 0x8000000000008002n, 0x8000000000000080n, 0x800an, 0x800000008000000an, 0x8000000080008081n, 0x8000000000008080n, 0x80000001n, 0x8000000080008008n];
const ROT = [0, 1, 62, 28, 27, 36, 44, 6, 55, 20, 3, 10, 43, 25, 39, 41, 45, 15, 21, 8, 18, 2, 61, 56, 14];
const M64 = (1n << 64n) - 1n;
const rot = (v, n) => (n === 0 ? v : ((v << BigInt(n)) | (v >> BigInt(64 - n))) & M64);
function keccakF(A) {
  for (let r = 0; r < 24; r++) {
    const C = [0, 1, 2, 3, 4].map((x) => A[x] ^ A[x + 5] ^ A[x + 10] ^ A[x + 15] ^ A[x + 20]);
    for (let x = 0; x < 5; x++) {
      const D = C[(x + 4) % 5] ^ rot(C[(x + 1) % 5], 1);
      for (let y = 0; y < 25; y += 5) A[x + y] ^= D;
    }
    const B = new Array(25);
    for (let x = 0; x < 5; x++) for (let y = 0; y < 5; y++) B[y + 5 * ((2 * x + 3 * y) % 5)] = rot(A[x + 5 * y], ROT[x + 5 * y]);
    for (let x = 0; x < 5; x++) for (let y = 0; y < 5; y++) A[x + 5 * y] = B[x + 5 * y] ^ (~B[((x + 1) % 5) + 5 * y] & M64 & B[((x + 2) % 5) + 5 * y]);
    A[0] ^= RC[r];
  }
}
export function keccak256(data) {
  const rate = 136;
  const msg = Buffer.from(data);
  const padded = Buffer.alloc(Math.ceil((msg.length + 1) / rate) * rate);
  msg.copy(padded);
  padded[msg.length] ^= 0x01;
  padded[padded.length - 1] ^= 0x80;
  const A = new Array(25).fill(0n);
  for (let off = 0; off < padded.length; off += rate) {
    for (let i = 0; i < rate / 8; i++) A[i] ^= padded.readBigUInt64LE(off + 8 * i);
    keccakF(A);
  }
  const out = Buffer.alloc(32);
  for (let i = 0; i < 4; i++) out.writeBigUInt64LE(A[i], 8 * i);
  return out;
}

// ------------------------------------------------------------------ checks
const hex = (s) => String(s ?? "").toLowerCase().replace(/^0x/, "");
const result = (id, status, reason, extra) => ({ id, status, reason, ...(extra ? { evidence: extra } : {}) });
const modelAtt = (report, model) => (report?.model_attestations ?? []).find((m) => m.model_name === model);

/** TDX quote v4: report_data is the 64 bytes at offset 48 (header) + 520 (TD report fields before it). */
export function tdxReportData(quoteHex) {
  const q = Buffer.from(hex(quoteHex), "hex");
  if (q.length < 632 || q.readUInt16LE(0) !== 4 || q.readUInt32LE(4) !== 0x81) return null;
  return q.subarray(568, 632);
}

export function localChecks(report, { model, nonce, signer }) {
  const m = modelAtt(report, model);
  if (!m) return [result("model_in_report", "fail", `no attestation for ${model} in the report`)];
  const addr = hex(m.signing_address);
  const out = [];
  out.push(addr === hex(signer)
    ? result("address_is_contract_signer", "pass", `the report's signing address ${m.signing_address} is the signer the contract checks (out/gates/g6.json)`)
    : result("address_is_contract_signer", "fail", `report signs as 0x${addr}, the contract trusts ${signer}`));
  const pk = Buffer.from(hex(m.signing_public_key), "hex");
  const derived = pk.length === 64 ? keccak256(pk).subarray(12).toString("hex") : pk.length === 65 && pk[0] === 4 ? keccak256(pk.subarray(1)).subarray(12).toString("hex") : null;
  out.push(derived === addr
    ? result("public_key_derives_address", "pass", "keccak256 of the reported public key gives the signing address")
    : result("public_key_derives_address", "fail", derived ? `public key gives 0x${derived}` : "public key is not 64 bytes"));
  out.push(hex(m.request_nonce) === hex(nonce)
    ? result("nonce_echoed", "pass", `the report answers our nonce ${nonce.slice(0, 16)}…`)
    : result("nonce_echoed", "fail", `report nonce ${String(m.request_nonce).slice(0, 16)}… is not ours: a replayed report`));
  const rd = tdxReportData(m.intel_quote);
  const want = addr.padEnd(40, "0") + "00".repeat(12) + hex(nonce);
  out.push(!rd
    ? result("tdx_report_data_binds_address_and_nonce", "fail", "not a TDX v4 quote")
    : rd.toString("hex") === want
      ? result("tdx_report_data_binds_address_and_nonce", "pass", "the TDX quote's report_data is the signing address, 12 zero bytes, then our nonce: the key was reported from inside the TD")
      : result("tdx_report_data_binds_address_and_nonce", "fail", `report_data ${rd.toString("hex").slice(0, 24)}… does not bind address and nonce`));
  let nv = null;
  try { nv = JSON.parse(m.nvidia_payload); } catch { /* absent or not JSON */ }
  out.push(!nv
    ? result("gpu_evidence_carries_nonce", "unverified", "no NVIDIA evidence in the report")
    : hex(nv.nonce) === hex(nonce)
      ? result("gpu_evidence_carries_nonce", "pass", `NVIDIA ${nv.arch} evidence for ${nv.evidence_list?.length ?? 0} GPU(s) was produced for our nonce`)
      : result("gpu_evidence_carries_nonce", "fail", "the GPU evidence answers another nonce"));
  return out;
}

async function post(url, body, timeoutMs = 60000) {
  const r = await fetch(url, { method: "POST", redirect: "error", headers: { "content-type": "application/json", accept: "application/json" }, body, signal: AbortSignal.timeout(timeoutMs) });
  const text = await r.text();
  return { status: r.status, text };
}

async function tdxQuote(m) {
  try {
    const { status, text } = await post(PHALA, JSON.stringify({ hex: m.intel_quote }));
    const j = JSON.parse(text);
    const body = j?.quote?.body ?? {};
    const rd = hex(body.reportdata ?? body.report_data ?? "");
    const agrees = !rd || rd === tdxReportData(m.intel_quote)?.toString("hex");
    if (status === 200 && j.success === true && agrees) {
      return result("tdx_quote_verifies", "pass", "Intel DCAP verification of the quote succeeded (Phala's public verifier, a third party)", { verifier: PHALA, id: j.id, success: j.success, header: j?.quote?.header });
    }
    return result("tdx_quote_verifies", "fail", `verifier answered ${status}, success ${j?.success}${agrees ? "" : ", and its report_data differs"}`, { verifier: PHALA });
  } catch (e) {
    return result("tdx_quote_verifies", "unverified", `verifier unreachable: ${String(e.message).slice(0, 120)}`);
  }
}

function b64url(s) { return Buffer.from(s.replace(/-/g, "+").replace(/_/g, "/"), "base64"); }

async function gpuNras(m, nonce) {
  if (!m.nvidia_payload) return result("gpu_attestation_nras", "unverified", "no NVIDIA evidence in the report");
  try {
    const { status, text } = await post(NRAS, m.nvidia_payload, 90000);
    if (status !== 200) return result("gpu_attestation_nras", "fail", `NRAS answered HTTP ${status}`);
    const arr = JSON.parse(text);
    const overall = arr?.[0]?.[0] === "JWT" ? arr[0][1] : null;
    if (!overall) return result("gpu_attestation_nras", "fail", "NRAS gave no overall token");
    const [h, p, s] = overall.split(".");
    const header = JSON.parse(b64url(h));
    const claims = JSON.parse(b64url(p));
    const jwks = await (await fetch(NRAS_JWKS, { redirect: "error", signal: AbortSignal.timeout(30000) })).json();
    const jwk = (jwks.keys ?? []).find((k) => k.kid === header.kid);
    const sigOk = Boolean(jwk) && edVerify("sha384", Buffer.from(`${h}.${p}`), { key: createPublicKey({ key: jwk, format: "jwk" }), dsaEncoding: "ieee-p1363" }, b64url(s));
    const ok = claims["x-nvidia-overall-att-result"] === true;
    const nonceOk = typeof claims.eat_nonce === "string" && hex(claims.eat_nonce) === hex(nonce);
    const evidence = { verifier: NRAS, iss: claims.iss, "x-nvidia-overall-att-result": claims["x-nvidia-overall-att-result"], eat_nonce: claims.eat_nonce ?? null, kid: header.kid, jwt_signature_valid: sigOk };
    if (sigOk && ok && nonceOk) return result("gpu_attestation_nras", "pass", "NVIDIA NRAS attested the GPU(s): overall result true, token signed by NVIDIA (JWKS), for our nonce", evidence);
    return result("gpu_attestation_nras", "fail", `NRAS: overall ${claims["x-nvidia-overall-att-result"]}, signature ${sigOk ? "valid" : "invalid"}, nonce ${nonceOk ? "ours" : "not ours"}`, evidence);
  } catch (e) {
    return result("gpu_attestation_nras", "unverified", `NRAS unreachable: ${String(e.message).slice(0, 120)}`);
  }
}

async function signerListed(addr, agents) {
  const seen = [];
  try {
    for (const agent of agents) {
      const r = await (await fetch(RPC, { method: "POST", redirect: "error", headers: { "content-type": "application/json" }, body: JSON.stringify({ jsonrpc: "2.0", id: 1, method: "query", params: { request_type: "call_function", finality: "final", account_id: agent, method_name: "get_status", args_base64: Buffer.from("{}").toString("base64") } }), signal: AbortSignal.timeout(20000) })).json();
      if (!r.result?.result) continue;
      const st = JSON.parse(Buffer.from(r.result.result).toString());
      if ((st.signers ?? []).map(hex).includes(hex(addr))) seen.push(agent);
    }
  } catch (e) {
    return result("address_in_testnet_signer_list", "unverified", `testnet RPC unreachable: ${String(e.message).slice(0, 120)}`);
  }
  return seen.length
    ? result("address_in_testnet_signer_list", "pass", `get_status.signers lists the address on ${seen.join(", ")}`, { agents: seen })
    : result("address_in_testnet_signer_list", "fail", `no checked agent lists ${addr}`, { checked: agents });
}

// ------------------------------------------------------------------ G7b: which code runs
// Published dstack OS releases (production images), copied from the meta-dstack
// v0.5.12 release notes: https://github.com/Dstack-TEE/meta-dstack/releases
export const PUBLISHED_OS_IMAGES = {
  "91fd5d69ccee3e5ea4f2aeb8c9b632c4358c4ecfc29ba1aa6fb1e402af209829": "dstack-nvidia-0.5.12",
  "8db40cb23008d6213668b5816702d100b37b859a8a5bfd18d938ae3a1b3715bd": "dstack-0.5.12",
};
export const OS_RELEASES_SOURCE = "https://github.com/Dstack-TEE/meta-dstack/releases (v0.5.12)";

const sha256 = (b) => createHash("sha256").update(b).digest("hex");
const sha384 = (b) => createHash("sha384").update(b).digest();

/** TDX quote v4 TD report fields (offset in the quote, 48-byte header first). */
function tdxRegisters(quoteHex) {
  const q = Buffer.from(hex(quoteHex), "hex");
  if (q.length < 632 || q.readUInt16LE(0) !== 4) return null;
  const f = (a) => q.subarray(a, a + 48).toString("hex");
  return { mrtd: f(184), mrconfigid: f(232), rtmr0: f(376), rtmr1: f(424), rtmr2: f(472), rtmr3: f(520) };
}

/** dstack's RTMR3 event digest: sha384(event_type as u32 LE || ":" || name || ":" || payload). */
function eventDigest(e) {
  return sha384(Buffer.concat([Buffer.from(Uint32Array.of(e.event_type).buffer), Buffer.from(`:${e.event}:`), Buffer.from(hex(e.event_payload), "hex")]));
}

export function codeChecks(report, { model }) {
  const m = modelAtt(report, model);
  if (!m) return [result("model_in_report", "fail", `no attestation for ${model} in the report`)];
  const tcb = typeof m.info?.tcb_info === "string" ? JSON.parse(m.info.tcb_info) : (m.info?.tcb_info ?? {});
  const regs = tdxRegisters(m.intel_quote);
  const out = [];
  if (!regs) return [result("quote_registers_match_report", "fail", "not a TDX v4 quote")];
  const names = ["mrtd", "rtmr0", "rtmr1", "rtmr2", "rtmr3"];
  const diff = names.filter((k) => regs[k] !== hex(tcb[k]));
  out.push(diff.length
    ? result("quote_registers_match_report", "fail", `the quote's ${diff.join(", ")} differ from the report's tcb_info`)
    : result("quote_registers_match_report", "pass", "MRTD and RTMR0-3 in the signed quote equal the report's tcb_info"));
  const appCompose = String(tcb.app_compose ?? "");
  const composeHash = hex(tcb.compose_hash ?? m.info?.compose_hash);
  const ok = sha256(Buffer.from(appCompose)) === composeHash && regs.mrconfigid === "01" + composeHash + "00".repeat(15);
  out.push(ok
    ? result("compose_is_measured", "pass", `sha256 of the report's app_compose (its docker compose) is compose_hash ${composeHash.slice(0, 16)}…, and the quote's MRCONFIGID is 01 || compose_hash: the quote measured exactly this compose`, { compose_hash: composeHash })
    : result("compose_is_measured", "fail", "the app_compose in the report is not the one the quote measured (sha256 or MRCONFIGID differs)"));
  const ev = (m.event_log ?? []).filter((e) => e.imr === 3);
  let r = Buffer.alloc(48);
  for (const e of ev) r = sha384(Buffer.concat([r, eventDigest(e)]));
  const payload = (name) => { const e = ev.find((x) => x.event === name); return e ? hex(e.event_payload) : null; };
  const osHash = hex(tcb.os_image_hash ?? m.info?.os_image_hash);
  const replay = r.toString("hex") === regs.rtmr3;
  const bound = payload("compose-hash") === composeHash && payload("os-image-hash") === osHash;
  out.push(replay && bound
    ? result("rtmr3_replays", "pass", `the ${ev.length} RTMR3 events, digests recomputed from their payloads, replay to the quote's RTMR3; its compose-hash and os-image-hash events name the hashes above`)
    : result("rtmr3_replays", "fail", replay ? "RTMR3 replays, but its compose-hash or os-image-hash event names another hash" : "the RTMR3 event log does not replay to the quote's RTMR3"));
  const release = PUBLISHED_OS_IMAGES[osHash];
  out.push(release
    ? result("os_image_is_published_dstack_release", "pass", `os_image_hash ${osHash.slice(0, 16)}… is the published ${release} image (${OS_RELEASES_SOURCE})`, { os_image_hash: osHash, release })
    : result("os_image_is_published_dstack_release", "fail", `os_image_hash ${osHash.slice(0, 16)}… is not in the published releases we compare with (${OS_RELEASES_SOURCE})`));
  let compose = {};
  try { compose = JSON.parse(appCompose); } catch { /* not JSON */ }
  const allowed = new Set(compose.allowed_envs ?? []);
  const images = [...String(compose.docker_compose_file ?? "").matchAll(/^\s*image:\s*(\S+)/gm)].map((x) => x[1]);
  const unpinned = images.filter((i) => !/@sha256:[0-9a-f]{64}/.test(i));
  const overridable = images.map((i) => /^\$\{([A-Z0-9_]+)/.exec(i)?.[1]).filter((v) => v && allowed.has(v));
  const evidence = { images, overridable_by_allowed_env: overridable };
  out.push(unpinned.length
    ? result("container_images_pinned", "fail", `${unpinned.length} of ${images.length} images are not pinned by digest`, evidence)
    : overridable.length
      ? result("container_images_pinned", "unverified", `all ${images.length} images are pinned by sha256, but ${overridable.join(", ")} is an allowed launch-time env that can replace an image`, evidence)
      : result("container_images_pinned", "pass", `all ${images.length} images in the measured compose are pinned by sha256 digest`, evidence));
  return out;
}

export async function checkAll(report, { model, nonce, signer, offline = false, agents = [] }) {
  const out = [...localChecks(report, { model, nonce, signer }), ...codeChecks(report, { model })];
  const m = modelAtt(report, model);
  if (!m) return out;
  if (offline) {
    out.push(result("tdx_quote_verifies", "unverified", "offline: Intel DCAP verification needs the network"));
    out.push(result("gpu_attestation_nras", "unverified", "offline: NVIDIA NRAS needs the network"));
    out.push(result("address_in_testnet_signer_list", "unverified", "offline: reading the contract needs the network"));
    return out;
  }
  out.push(await tdxQuote(m));
  out.push(await gpuNras(m, nonce));
  out.push(await signerListed(m.signing_address, agents));
  return out;
}

// ------------------------------------------------------------------ CLI
function g6() {
  const j = JSON.parse(readFileSync("out/gates/g6.json", "utf8"));
  const s = (j.samples ?? [])[0] ?? {};
  if (!/^0x[0-9a-fA-F]{40}$/.test(s.signing_address ?? "") || !s.model) throw new Error("out/gates/g6.json has no signing_address and model");
  return { signer: s.signing_address, model: s.model };
}

function testnetAgents() {
  try {
    const d = JSON.parse(readFileSync("out/testnet/deploy.json", "utf8"));
    return [`translator3.${d.root}`, `translator.${d.root}`];
  } catch { return []; }
}

function print(checks) {
  for (const c of checks) console.log(`${c.status.toUpperCase().padEnd(10)} ${c.id.padEnd(42)} ${c.reason}`);
  const fails = checks.filter((c) => c.status === "fail").length;
  console.log(`attest: ${checks.filter((c) => c.status === "pass").length} pass, ${fails} fail, ${checks.filter((c) => c.status === "unverified").length} unverified`);
  return fails;
}

async function main() {
  const [cmd, flag] = process.argv.slice(2);
  const { signer, model } = g6();
  if (cmd === "fetch") {
    const nonce = randomBytes(32).toString("hex");
    const url = `${BASE}/attestation/report?model=${encodeURIComponent(model)}&signing_algo=ecdsa&nonce=${nonce}`;
    const r = await fetch(url, { redirect: "error", signal: AbortSignal.timeout(60000) });
    if (r.status !== 200) throw new Error(`attestation report: HTTP ${r.status}`);
    const report = await r.json();
    const fetched_at = new Date().toISOString();
    const checks = await checkAll(report, { model, nonce, signer, agents: testnetAgents() });
    writeFileSync(OUT, JSON.stringify({
      schema: "nearai-attestation-check.v1",
      what: "Is the key that signs NEAR AI 'verified' payments (gate G6) in an Intel TDX + NVIDIA GPU TEE? Report fetched from NEAR AI Cloud (no completion, no charge), checked locally and by public verifiers.",
      fetched_at, endpoint: url, model, nonce, contract_signer: signer,
      not_checked: "MRTD and RTMR0-2 are not compared with a reference: nobody publishes them, and they can only be computed by building the dstack image (dstack-mr measure). The compose's sha256-pinned images are not traced to source, and the model-serving containers the compose-manager launches are not in the measured compose",
      checks, report,
    }, null, 2) + "\n");
    console.log(`attest: wrote ${OUT} (fetched ${fetched_at}, nonce ${nonce.slice(0, 16)}…)`);
    process.exit(print(checks) ? 1 : 0);
  }
  if (cmd === "check") {
    const saved = JSON.parse(readFileSync(OUT, "utf8"));
    if (hex(saved.contract_signer) !== hex(signer)) console.log(`note: the saved report was checked against ${saved.contract_signer}; out/gates/g6.json now names ${signer}`);
    const checks = await checkAll(saved.report, { model: saved.model, nonce: saved.nonce, signer, offline: flag === "--offline", agents: testnetAgents() });
    console.log(`attest: ${OUT}, fetched ${saved.fetched_at}${flag === "--offline" ? " (offline)" : ""}`);
    process.exit(print(checks) ? 1 : 0);
  }
  console.error("usage: verify-nearai-attestation.mjs fetch | check [--offline]");
  process.exit(2);
}

if (process.argv[1] && fileURLToPath(import.meta.url) === process.argv[1]) {
  main().catch((e) => { console.error(`attest: ${e.message}`); process.exit(1); });
}
