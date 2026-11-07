#!/usr/bin/env node
// Writes fixtures/mcp/sealed.json: two illustrative sold agents with a sealed
// config already delivered, for the offline sealed_delivery tool. One seller
// delivered the committed bundle, the other a different prompt of the same
// length. The buyer's one-time secret is a fixed DEMO value (fixture only;
// on testnet the secret lives in the 0600 file buy_agent wrote).
import { writeFileSync } from "node:fs";
import { commit, publicKey, seal } from "./sealed-config.mjs";

const bundle = Buffer.from(JSON.stringify({ schema: "agent-config.v1", system_prompt: "You translate EN<->ES. Keep names and numbers exact.", model: "deepseek-v3", tools: ["glossary"], rotate_after_sale: ["NEARAI_API_KEY"] }));
const fake = Buffer.from(bundle);
fake[50] = "X".charCodeAt(0);
const buyerSk = Buffer.alloc(32, 42);
const pk = publicKey(buyerSk);
const T = 1_767_225_600_000_000_000n;
const DAY = 86_400_000_000_000n;
const entry = (b, eph) => {
  const s = seal(Buffer.alloc(32, eph), pk, b);
  return {
    sale_id: 1, seller: "alice.testnet", buyer: "summarizer.testnet", price: "1000000000", upfront: "700000000", escrow: "300000000",
    commit: commit(bundle), delivery_pk: pk.toString("hex"),
    deliver_by: String(T + 2n * DAY), dispute_until: String(T + 8n * DAY), cleared: false,
    state: { Delivered: { ephemeral_pk: s.ephemeral_pk, cipher_hash: null, at: String(T + DAY) } },
    ciphertext: s.ciphertext.toString("base64"),
  };
};
const out = {
  schema: "mcp-sealed.v1",
  note: "Illustrative offline fixtures for sealed_delivery (scripts/gen-sealed-fixture.mjs). brainy.testnet delivered the committed bundle; swapped.testnet delivered a different prompt. demo_buyer_sk is a fixed demo secret for these fixtures only.",
  now: String(T + 2n * DAY),
  demo_buyer_sk: buyerSk.toString("hex"),
  agents: { "brainy.testnet": entry(bundle, 17), "swapped.testnet": entry(fake, 18) },
};
writeFileSync(new URL("../fixtures/mcp/sealed.json", import.meta.url), JSON.stringify(out, null, 2) + "\n");
console.log("wrote fixtures/mcp/sealed.json");
