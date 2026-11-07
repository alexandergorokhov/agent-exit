// SC13: buy_agent on a listing that sells a sealed config makes a one-time
// X25519 delivery key, stores the secret 0600 before sending, and returns
// only the public half. The key opens what the seller seals to it.
import { test } from "node:test";
import assert from "node:assert/strict";
import { mkdtempSync, readFileSync, statSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { valid, world, writer } from "./helpers.js";

test("SC13 a sealed-config listing: the buy names a one-time key; the secret stays in a 0600 file", async () => {
  const dir = mkdtempSync(join(tmpdir(), "agent-exit-sc13-"));
  const w = structuredClone(world);
  const commit = { hash: "a".repeat(64), bytes: 120 };
  w.agents["translator.testnet"].listing.config = commit;
  const wf = join(dir, "world.json");
  writeFileSync(wf, JSON.stringify(w));
  const s = await writer("summarizer.testnet", { AGENT_EXIT_WORLD: wf, AGENT_EXIT_SPEND_FILE: join(dir, "spend.json") });
  try {
    const r = await s.call("buy_agent", { agent: "translator.testnet", max_price: 1000, confirm: true });
    assert.ok(valid("buy_agent", r), JSON.stringify(r.body));
    const sc = (r.body as any).sealed_config;
    assert.equal(sc.hash, commit.hash);
    assert.match(sc.delivery_pk, /^[0-9a-f]{64}$/);
    assert.ok(!JSON.stringify(r.body).includes("delivery_sk"), "the secret is never in the tool output");
    assert.equal(statSync(sc.delivery_key_file).mode & 0o777, 0o600);
    const saved = JSON.parse(readFileSync(sc.delivery_key_file, "utf8"));
    // The saved secret is the key the seller seals to (same cipher as the contract).
    const m = await import("../../../scripts/sealed-config.mjs" as string);
    assert.equal(m.publicKey(Buffer.from(saved.delivery_sk, "hex")).toString("hex"), sc.delivery_pk);
    const bundle = Buffer.from('{"system_prompt":"x"}');
    const sealed = m.seal(Buffer.alloc(32, 9), Buffer.from(sc.delivery_pk, "hex"), bundle);
    assert.deepEqual(m.open(Buffer.from(saved.delivery_sk, "hex"), Buffer.from(sealed.ephemeral_pk, "hex"), sealed.ciphertext), bundle);
  } finally {
    await s.close();
  }
});
