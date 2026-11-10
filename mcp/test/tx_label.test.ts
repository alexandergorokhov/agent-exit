// TL1 (judge-testnet K7, 2026-11-10): the recorder labels action batches
// "create_account" (CreateAccount + Transfer + AddKey) and "deploy+new"
// (DeployContract + FunctionCall new). Re-reading a transaction must give the
// same label, or a real transaction shows as "differs" on the page and in
// check_claims.
import { test } from "node:test";
import assert from "node:assert/strict";
import { txOutcomeOf } from "../src/backend.js";

const tx = (actions: unknown[]) => ({ transaction: { signer_id: "a.testnet", receiver_id: "b.a.testnet", actions }, status: { SuccessValue: "" }, receipts_outcome: [] });

test("TL1 a plain function call keeps its method name", () => {
  assert.equal(txOutcomeOf(tx([{ FunctionCall: { method_name: "withdraw" } }])).method, "withdraw");
});

test("TL1 CreateAccount + Transfer + AddKey reads as create_account", () => {
  assert.equal(txOutcomeOf(tx(["CreateAccount", { Transfer: { deposit: "1" } }, { AddKey: {} }])).method, "create_account");
  assert.equal(txOutcomeOf(tx([{ CreateAccount: {} }, { Transfer: { deposit: "1" } }])).method, "create_account");
});

test("TL1 DeployContract + FunctionCall new reads as deploy+new", () => {
  assert.equal(txOutcomeOf(tx([{ DeployContract: { code: "AA" } }, { FunctionCall: { method_name: "new" } }])).method, "deploy+new");
  assert.equal(txOutcomeOf(tx([{ DeployContract: { code: "AA" } }])).method, "deploy");
});

test("TL1 a lone DeleteKey or AddKey reads as delete_key or add_key", () => {
  assert.equal(txOutcomeOf(tx([{ DeleteKey: { public_key: "ed25519:x" } }])).method, "delete_key");
  assert.equal(txOutcomeOf(tx([{ AddKey: { public_key: "ed25519:x" } }])).method, "add_key");
});

test("TL1 an empty batch is (no call)", () => {
  assert.equal(txOutcomeOf(tx([])).method, "(no call)");
});
