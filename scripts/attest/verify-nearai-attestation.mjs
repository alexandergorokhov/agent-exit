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
// Not checked: which code the TEE runs (the measurements are recorded, not compared
// with a published reference build). The report costs nothing: no completion is made.
import { createPublicKey, randomBytes, verify as edVerify } from "node:crypto";
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

export async function checkAll(report, { model, nonce, signer, offline = false, agents = [] }) {
  const out = localChecks(report, { model, nonce, signer });
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
      not_checked: "which code the TEE runs: the measurements (mrtd, rtmr0-3, compose_hash, os_image_hash) are recorded in the report but not compared with a published reference build",
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
