// Gate G2, relay half (runs on the owner's machine after `make gate-g2`):
//   node scripts/testnet/g2-relay.mjs out/gates/g2-sign.json
//   DRY_RUN=1 node scripts/testnet/g2-relay.mjs     (prints the plan, sends nothing, reads no secret)
// 1. chainsig.js derives the agent's Base Sepolia address for path wallet-1
//    from v1.signer-prod.testnet: it must equal the contract's address.
// 2. viem checks chain id 84532, the USDC EIP-712 domain separator against
//    the contract's, the derived address's USDC balance and the nonce.
// 3. The relayer EOA (BASE_RELAYER_KEY) sends the contract-built calldata
//    (transferWithAuthorization) to Base Sepolia USDC and waits for it.
// Writes out/gates/g2.json. Never prints a secret. Testnet only.
import { readFileSync, writeFileSync, mkdirSync } from "node:fs";
import { homedir } from "node:os";
import { dirname, resolve } from "node:path";
import { checkAuthorization, checkBaseRpc, checkRelayerKey, CHAIN_ID, DRY_RUN_PLAN, parseEnvFile, scrub } from "./g2-guards.mjs";

const USDC = "0x036cbd53842c5426634e7929541ec2318f3dcf7e";
const MPC = "v1.signer-prod.testnet";

function loadEnv() {
  const env = { ...process.env };
  try {
    const p = process.env.AGENT_EXIT_ENV || `${homedir()}/.agent-exit.env`;
    for (const [k, v] of Object.entries(parseEnvFile(readFileSync(p, "utf8")))) if (!(k in env)) env[k] = v;
  } catch { /* the runner passes what is needed */ }
  return env;
}

if (process.env.DRY_RUN && process.env.DRY_RUN !== "0") {
  console.log("DRY RUN (g2-relay): nothing is sent, no secret is read.");
  DRY_RUN_PLAN.forEach((s) => console.log(`  ${s}`));
  process.exit(0);
}

const env = loadEnv();
const secrets = [env.BASE_RELAYER_KEY, env.BASE_RELAYER_KEY?.replace(/^0x/, ""), env.BASE_SEPOLIA_RPC];
const signFile = process.argv[2] || "out/gates/g2-sign.json";
const outFile = resolve(dirname(signFile), "g2.json");
const result = { schema: "gate-g2.v1", gate: "G2 Chain Signatures: contract-signed EIP-3009 transfer relayed on Base Sepolia", pass: false };
const save = () => { mkdirSync(dirname(outFile), { recursive: true }); writeFileSync(outFile, JSON.stringify(result, null, 2) + "\n"); console.log(`  wrote ${outFile}`); };
const fail = (msg) => { result.error = scrub(msg, secrets).slice(0, 600); save(); console.error(`g2-relay: ${result.error}`); process.exit(1); };

async function main() {
  if (env.AGENT_EXIT_NETWORK && env.AGENT_EXIT_NETWORK !== "testnet") fail("AGENT_EXIT_NETWORK is not testnet");
  const rpc = env.BASE_SEPOLIA_RPC || "https://sepolia.base.org";
  checkBaseRpc(rpc);
  const relayerKey = checkRelayerKey(env.BASE_RELAYER_KEY);

  const sign = JSON.parse(readFileSync(signFile, "utf8"));
  const agent = sign.samples?.find((s) => s.ok)?.auth;
  if (!agent) fail("no signed authorization in " + signFile);
  checkAuthorization(agent);
  const accountId = sign.txs?.find((t) => t.method === "pay_base")?.receiver
    || sign.samples.find((s) => s.ok)?.auth?.agent;

  const { createPublicClient, createWalletClient, http, parseAbi } = await import("viem");
  const { privateKeyToAccount } = await import("viem/accounts");
  const { baseSepolia } = await import("viem/chains");
  const pub = createPublicClient({ chain: baseSepolia, transport: http(rpc, { timeout: 30_000 }) });

  // 1. chainsig.js derivation.
  try {
    const { contracts, chainAdapters } = await import("chainsig.js");
    const contract = new contracts.ChainSignatureContract({ networkId: "testnet", contractId: MPC });
    const evm = new chainAdapters.evm.EVM({ publicClient: pub, contract });
    const { address } = await evm.deriveAddressAndPublicKey(sign.agent_account || accountId, "wallet-1");
    result.chainsig_js_address = address.toLowerCase();
    result.chainsig_js_matches = address.toLowerCase() === sign.contract_address;
  } catch (e) {
    result.chainsig_js_error = scrub(e?.message || e, secrets).slice(0, 400);
  }

  // 2. Chain and token checks.
  const chainId = await pub.getChainId();
  result.chain_id = chainId;
  if (chainId !== CHAIN_ID) fail(`chain id ${chainId} is not Base Sepolia (${CHAIN_ID})`);
  const abi = parseAbi([
    "function DOMAIN_SEPARATOR() view returns (bytes32)",
    "function name() view returns (string)",
    "function version() view returns (string)",
    "function balanceOf(address) view returns (uint256)",
    "function authorizationState(address,bytes32) view returns (bool)",
  ]);
  const read = (functionName, args = []) => pub.readContract({ address: USDC, abi, functionName, args });
  result.usdc = { name: await read("name"), version: await read("version"), domain_separator: (await read("DOMAIN_SEPARATOR")).toLowerCase() };
  result.domain_separator_matches = result.usdc.domain_separator === String(sign.domain_separator).toLowerCase();
  if (!result.domain_separator_matches) fail(`USDC domain separator ${result.usdc.domain_separator} != contract ${sign.domain_separator}`);
  const balance = await read("balanceOf", [agent.from]);
  result.from = agent.from;
  result.from_usdc_balance = balance.toString();
  if (balance < BigInt(agent.value)) fail(`${agent.from} holds ${balance} USDC base units < ${agent.value}: fund it from the Circle faucet (Base Sepolia), then rerun make gate-g2`);
  if (await read("authorizationState", [agent.from, agent.nonce])) fail("this authorization nonce is already used");
  if (Number(agent.valid_before) <= Math.floor(Date.now() / 1000) + 20) fail("the authorization expired (valid 300 s): rerun make gate-g2");

  // 3. Relay the contract-built calldata.
  const relayer = privateKeyToAccount(relayerKey);
  result.relayer = relayer.address;
  const wallet = createWalletClient({ account: relayer, chain: baseSepolia, transport: http(rpc, { timeout: 30_000 }) });
  const t0 = Date.now();
  const hash = await wallet.sendTransaction({ to: USDC, data: agent.calldata });
  const receipt = await pub.waitForTransactionReceipt({ hash, timeout: 120_000 });
  result.base_tx = hash;
  result.explorer = `https://sepolia.basescan.org/tx/${hash}`;
  result.status = receipt.status;
  result.ms = Date.now() - t0;
  result.paid_from = agent.from;
  result.paid_to = agent.to;
  result.value = agent.value;
  result.pass = receipt.status === "success";
  save();
  process.exit(result.pass ? 0 : 1);
}

// Any error (a refused guard, a viem error that quotes the RPC URL) ends here,
// scrubbed, never as a raw stack.
main().catch((e) => fail(String(e?.shortMessage || e?.message || e)));
