// "Who else can take this account?": the page's version of the MCP tool
// check_account (mcp/src/tools.ts), the same rules on the same facts. Pure:
// no network here. web/account.js reads the facts through the read-only relay;
// mcp/test/account_parity.test.ts checks both give the same verdict.

export const EMPTY_CODE = "11111111111111111111111111111111";
// tools/scan UPGRADE_WORDS plus check_account's extra words.
export const CODE_WORDS = ["upgrade", "deploy", "migrate", "set_code", "update_contract", "add_key", "add_full_access_key", "delete_account", "stage_code",
  "wasm", "self_update", "replace_code", "set_contract", "update_code", "redeploy", "set_owner_key"];

const clean = (s) => String(s).replace(/[\p{Cc}\p{Cf}\p{Co}\p{Cn}\p{Zl}\p{Zp}]/gu, " ").replace(/\s+/g, " ").trim().slice(0, 120);

/** Function export names of a wasm module; throws on a malformed one. */
export function wasmExports(code) {
  if (code.length < 8) throw new Error("malformed wasm");
  let i = 8;
  let limit = code.length;
  const byte = () => { if (i >= limit) throw new Error("malformed wasm"); return code[i++]; };
  const leb = () => {
    let r = 0;
    for (let shift = 0; shift < 35; shift += 7) {
      const b = byte();
      r += (b & 0x7f) * 2 ** shift;
      if ((b & 0x80) === 0) return r;
    }
    throw new Error("malformed wasm");
  };
  const names = [];
  while (i < code.length) {
    limit = code.length;
    const id = byte();
    const size = leb();
    const end = i + size;
    if (end > code.length) throw new Error("malformed wasm");
    if (id === 7) {
      limit = end;
      const n = leb();
      // Review 6 L4: never return a short list that could hide an export.
      if (n > 10000) throw new Error("too many exports to check");
      for (let k = 0; k < n; k++) {
        const len = leb();
        if (i + len > end) throw new Error("malformed wasm");
        const name = new TextDecoder().decode(code.subarray(i, i + len));
        i += len;
        const kind = byte();
        leb();
        if (kind === 0) names.push(name);
      }
    }
    i = end;
  }
  return names;
}

/**
 * facts: { account, code_hash, methods: string[] | null (null = unreadable),
 *          keys: [{ public_key, permission: "FullAccess" | { receiver_id, method_names, allowance } }],
 *          known_keys: string[] | null (the contract's own list, published build only) }
 * opts:  { publishedHash }
 * Returns { verdict, summary, findings: [{code, severity, text}], keys: [{public_key, can}] }.
 */
export function assess(facts, opts = {}) {
  const findings = [];
  // Review 4 M2: a NEAR global contract by owner account id can be swapped by that owner.
  const globalBy = facts.global_contract_account_id || null;
  const hasCode = facts.code_hash !== EMPTY_CODE || !!globalBy || !!facts.global_contract_hash;
  // Review 6 L5: a global contract's code is not the account's own, so its
  // methods were never read here.
  if (globalBy || facts.global_contract_hash) facts = { ...facts, methods: null };
  if (globalBy) findings.push({ code: "global_contract_by_account", severity: "high", text: `runs the global contract of ${clean(globalBy)}: that account can replace the code at any time` });
  const keys = facts.keys.map((k) => {
    const pk = clean(k?.public_key);
    if (k?.permission === "FullAccess") return { public_key: pk, full: true, can: "everything: new code, new keys, all NEAR, without the contract seeing it" };
    const p = k?.permission;
    if (!p || typeof p !== "object") {
      findings.push({ code: "unreadable_key", severity: "high", text: `${pk}: permission unreadable, treated as full access` });
      return { public_key: pk, full: true, can: "unknown: assume everything" };
    }
    const methods = (Array.isArray(p.method_names) ? p.method_names : []).map(clean);
    const receiver = clean(p.receiver_id);
    if (!methods.length) findings.push({ code: "fc_key_any_method", severity: "medium", text: `${pk} may call every method of ${receiver}` });
    if (p.allowance === null || p.allowance === undefined) findings.push({ code: "fc_key_no_allowance", severity: "low", text: `${pk} has no gas allowance cap` });
    if (receiver !== facts.account) findings.push({ code: "fc_key_other_contract", severity: "medium", text: `${pk} spends this account's NEAR on gas calling ${receiver}` });
    return { public_key: pk, full: false, can: `call ${methods.length ? methods.join(", ") : "EVERY method"} on ${receiver}` };
  });
  const full = keys.filter((k) => k.full).length;
  if (full && hasCode) findings.push({ code: "full_access_key_on_contract", severity: "high", text: `${full} full-access key(s) on an account with code: whoever holds one can replace the code or drain it` });
  if (full && !hasCode) findings.push({ code: "full_access_wallet", severity: "info", text: `an ordinary wallet: ${full} full-access key(s) control it` });
  if (hasCode && facts.methods === null) findings.push({ code: "code_unreadable", severity: "medium", text: "the code could not be read, so upgrade methods cannot be ruled out" });
  const upgrade = (facts.methods ?? []).filter((m) => CODE_WORDS.some((w) => m.toLowerCase().includes(w))).map(clean);
  if (upgrade.length) findings.push({ code: "code_change_method", severity: "high", text: `the contract exports ${upgrade.join(", ")}: a way to change the code or keys later` });
  const published = hasCode && opts.publishedHash && facts.code_hash === opts.publishedHash;
  if (published) {
    if (!Array.isArray(facts.known_keys)) findings.push({ code: "contract_keys_unreadable", severity: "high", text: "the contract's own key list could not be read" });
    else for (const k of facts.keys) if (!facts.known_keys.includes(String(k?.public_key))) findings.push({ code: "key_not_issued_by_contract", severity: "high", text: `${clean(k?.public_key)} was never installed by the contract` });
  }
  const has = (...c) => findings.some((f) => c.includes(f.code));
  const verdict = !hasCode ? "key_controlled"
    : full || upgrade.length || has("key_not_issued_by_contract", "contract_keys_unreadable", "global_contract_by_account") ? "backdoor_risk"
      : published && !has("fc_key_any_method", "fc_key_no_allowance", "unreadable_key", "fc_key_other_contract", "code_unreadable") ? "locked"
        : "no_key_unknown_code";
  const summary = {
    locked: "Locked: no key can change the code or take the account, and the code is the published Agent Exit build.",
    no_key_unknown_code: "No full-access key, but the code is not a build we can check: read its source.",
    key_controlled: "An ordinary account: whoever holds a full-access key controls it.",
    backdoor_risk: "Backdoor risk: code runs here, but a key or an upgrade path can override it.",
  }[verdict];
  return { verdict, summary, findings, keys: keys.map(({ public_key, can }) => ({ public_key, can })) };
}
