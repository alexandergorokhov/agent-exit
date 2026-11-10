// near-key-check: "who else can take this NEAR account?"
import { assess } from "./account-check.mjs";
import { readFacts } from "./read.mjs";
export { assess, wasmExports, CODE_WORDS, EMPTY_CODE } from "./account-check.mjs";
export { readFacts, TESTNET_RPC } from "./read.mjs";

/** Read `account` on testnet and assess it. `publishedHash`: the code hash
 *  you trust (e.g. the Agent Exit reproducible build); only then can the
 *  verdict be `locked`. */
export async function checkAccount(account, { publishedHash, rpc, fetchImpl } = {}) {
  const facts = await readFacts(account, { rpc, fetchImpl });
  return { account, code_hash: facts.code_hash, ...assess(facts, { publishedHash }) };
}

/** Guard for an agent about to pay or buy another agent: throws unless the
 *  account is `locked` (or one of `allow`). */
export async function assertNoBackdoor(account, { allow = ["locked"], ...opts } = {}) {
  const r = await checkAccount(account, opts);
  if (!allow.includes(r.verdict)) {
    const e = new Error(`${account}: ${r.verdict}: ${r.summary}`);
    e.result = r;
    throw e;
  }
  return r;
}
