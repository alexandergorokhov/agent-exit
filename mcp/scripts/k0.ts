// K0 for `make judge`: the code hash of the committed wasm
// (contracts/res/agent_exit.wasm) is the one every record names: the page's
// testnet data, the testnet deploy record, the testnet MCP entry and
// SHA256SUMS. Offline: it reads files only. On testnet, the two printed
// commands compare it with the deployed account.
import { createHash } from "node:crypto";
import { readFileSync } from "node:fs";
import { join } from "node:path";

const B58 = "123456789ABCDEFGHJKLMNPQRSTUVWXYZabcdefghijkmnopqrstuvwxyz";
export function base58(bytes: Uint8Array): string {
  let n = BigInt(`0x${Buffer.from(bytes).toString("hex") || "0"}`);
  let s = "";
  while (n > 0n) { s = B58[Number(n % 58n)] + s; n /= 58n; }
  for (const b of bytes) { if (b) break; s = `1${s}`; }
  return s;
}

export interface K0 {
  pass: boolean;
  hash: string;
  hex: string;
  mismatches: string[];
  account: string;
}

/** Paths are relative to `repo`; `files` lets a test point one record at a corrupted copy. */
export function checkCodeHash(repo: string, files: Partial<Record<"wasm" | "web" | "deploy" | "mcp" | "sums", string>> = {}): K0 {
  const f = { wasm: "contracts/res/agent_exit.wasm", web: "web/data/testnet.json", deploy: "out/testnet/deploy.json", mcp: ".mcp.json", sums: "contracts/res/SHA256SUMS", ...files };
  const read = (p: string) => readFileSync(p.startsWith("/") ? p : join(repo, p));
  const digest = createHash("sha256").update(read(f.wasm)).digest();
  const hash = base58(digest);
  const hex = digest.toString("hex");
  const json = (p: string) => JSON.parse(read(p).toString("utf8"));
  const sums = read(f.sums).toString("utf8").split("\n").find((l) => /\sagent_exit\.wasm$/.test(l.trim()))?.split(/\s+/)[0];
  const seen: [string, unknown][] = [
    [`${f.web} agent_code_hash`, json(f.web).agent_code_hash],
    [`${f.deploy} agent_code_hash`, json(f.deploy).agent_code_hash],
    [`${f.mcp} agent-exit-testnet AGENT_EXIT_CODE_HASH`, json(f.mcp).mcpServers?.["agent-exit-testnet"]?.env?.AGENT_EXIT_CODE_HASH],
  ];
  const mismatches = seen.filter(([, v]) => v !== hash).map(([k, v]) => `${k} is ${String(v)}`);
  if (sums !== hex) mismatches.push(`${f.sums} agent_exit.wasm is ${String(sums)}`);
  const account = (json(f.web).steps?.["honest-sale"]?.txs || []).find((x: { receiver?: string }) => /^translator\./.test(x.receiver || ""))?.receiver || "translator.agentexit5.testnet";
  return { pass: mismatches.length === 0, hash, hex, mismatches, account };
}

/** The two commands that compare the hash with the deployed account (read-only, public RPC). */
export function k0Commands(k: K0): string[] {
  return [
    `curl -s https://rpc.testnet.near.org -H 'Content-Type: application/json' -d '{"jsonrpc":"2.0","id":1,"method":"query","params":{"request_type":"view_account","finality":"final","account_id":"${k.account}"}}'   # result.code_hash = ${k.hash}`,
    `near contract call-function as-read-only ${k.account} contract_source_metadata json-args '{}' network-config testnet now   # NEP-330: build image and source commit`,
  ];
}
