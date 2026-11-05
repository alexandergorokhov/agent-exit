// M12 (review 5, M1): one RPC node cannot vouch for the code alone. The scan
// input's code hash is the hash of the bytes the server actually read and
// parsed, and both providers must report that same hash; otherwise the hash
// handed to the scanner cannot match the expected one, and the buy is blocked.
import { test } from "node:test";
import assert from "node:assert/strict";
import { createHash } from "node:crypto";
import { TestnetBackend } from "../src/backend.js";
import { loadConfig } from "../src/config.js";
import { b58encode } from "../src/nearsign.js";

const AGENT = "translator.testnet";
const RPC1 = "https://rpc.testnet.near.org";
const RPC2 = "https://test.rpc.fastnear.com";
const CODE = Buffer.from([0, 0x61, 0x73, 0x6d, 1, 0, 0, 0]);
const REAL = b58encode(createHash("sha256").update(CODE).digest());

function chain(hashFor: Record<string, string>, code: Buffer = CODE) {
  return (async (url: string, init?: RequestInit) => {
    const p = JSON.parse(String(init?.body)).params;
    const view = (v: unknown) => ({ result: [...Buffer.from(JSON.stringify(v))] });
    let result: unknown;
    if (p.request_type === "call_function") {
      if (p.method_name === "get_status") {
        result = view({ status: "Listed", owner: "seller.testnet", listing_version: 1, known_keys: [], known_keys_hash: "00", open_approvals: 0, frozen: false, admin: "admin.testnet", token: "xusd.testnet", paid_to_total: 0 });
      } else if (p.method_name === "get_listing") result = view(null);
      else return new Response(JSON.stringify({ result: { error: "MethodNotFound" } }), { status: 200 });
    } else if (p.request_type === "view_access_key_list") result = { keys: [] };
    else if (p.request_type === "view_account") result = { code_hash: hashFor[url] };
    else result = { code_base64: code.toString("base64") };
    return new Response(JSON.stringify({ result }), { status: 200 });
  }) as unknown as typeof fetch;
}

const input = async (f: typeof fetch) =>
  (await new TestnetBackend(loadConfig({ AGENT_EXIT_NETWORK: "testnet" }), f).scanInput(AGENT, {})) as { chain: { code_hash: string } };

test("M12 both providers report the hash of the bytes read: it is passed on", async () => {
  const i = await input(chain({ [RPC1]: REAL, [RPC2]: REAL }));
  assert.equal(i.chain.code_hash, REAL);
});

test("M12 a node that reports a different hash than the bytes it served cannot pass", async () => {
  const pinned = "Aots3gb33Yjm2pnhgdMYaMdDDk4ZRDnTgsaUA66nDDXq";
  const i = await input(chain({ [RPC1]: pinned, [RPC2]: pinned }));
  assert.notEqual(i.chain.code_hash, pinned);
});

test("M12 providers that disagree on the hash cannot pass", async () => {
  const i = await input(chain({ [RPC1]: REAL, [RPC2]: "11111111111111111111111111111111" }));
  assert.notEqual(i.chain.code_hash, REAL);
});
