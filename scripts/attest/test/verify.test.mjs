// G7: the NEAR AI Cloud attestation check, offline. Runs the verifier on the
// saved report (out/gates/g7-attestation.json): the local checks must pass, the
// network ones are "unverified" offline, and any tampering fails.
// Run: node --test scripts/attest/test/verify.test.mjs
import { test } from "node:test";
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { keccak256, localChecks, checkAll } from "../verify-nearai-attestation.mjs";

const saved = JSON.parse(readFileSync(new URL("../../../out/gates/g7-attestation.json", import.meta.url), "utf8"));
const G6_SIGNER = "0xfe0071895500a9755c18044948ab4f4e1a7f337e";
const byId = (checks) => Object.fromEntries(checks.map((c) => [c.id, c]));

test("G7 keccak-256 known vectors", () => {
  assert.equal(Buffer.from(keccak256(Buffer.alloc(0))).toString("hex"), "c5d2460186f7233c927e7db2dcc703c0e500b653ca82273b7bfad8045d85a470");
  assert.equal(Buffer.from(keccak256(Buffer.from("abc"))).toString("hex"), "4e03657aea45a94fc7d47ba826c8d667c0d1e6e33a64a036ec44f58fa12d6c45");
});

test("G7 the saved report passes every local check for the contract's signer", () => {
  const c = byId(localChecks(saved.report, { model: saved.model, nonce: saved.nonce, signer: G6_SIGNER }));
  for (const id of ["address_is_contract_signer", "public_key_derives_address", "nonce_echoed", "tdx_report_data_binds_address_and_nonce", "gpu_evidence_carries_nonce"]) {
    assert.equal(c[id].status, "pass", `${id}: ${c[id].reason}`);
  }
});

test("G7 offline, the network checks are unverified, never pass", async () => {
  const c = byId(await checkAll(saved.report, { model: saved.model, nonce: saved.nonce, signer: G6_SIGNER, offline: true }));
  assert.equal(c.tdx_quote_verifies.status, "unverified");
  assert.equal(c.gpu_attestation_nras.status, "unverified");
});

test("G7 a different signer fails", () => {
  const c = byId(localChecks(saved.report, { model: saved.model, nonce: saved.nonce, signer: "0x0000000000000000000000000000000000000001" }));
  assert.equal(c.address_is_contract_signer.status, "fail");
});

test("G7 a quote whose report_data does not bind the address fails", () => {
  const r = structuredClone(saved.report);
  const m = r.model_attestations.find((x) => x.model_name === saved.model);
  const q = Buffer.from(m.intel_quote, "hex");
  q[568] ^= 0xff;
  m.intel_quote = q.toString("hex");
  const c = byId(localChecks(r, { model: saved.model, nonce: saved.nonce, signer: G6_SIGNER }));
  assert.equal(c.tdx_report_data_binds_address_and_nonce.status, "fail");
});

test("G7 a replayed report (another nonce) fails", () => {
  const c = byId(localChecks(saved.report, { model: saved.model, nonce: "00".repeat(32), signer: G6_SIGNER }));
  assert.equal(c.nonce_echoed.status, "fail");
  assert.equal(c.tdx_report_data_binds_address_and_nonce.status, "fail");
});
