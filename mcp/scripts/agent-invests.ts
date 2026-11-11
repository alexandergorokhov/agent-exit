// Scripted "an agent invests" (AI1): an investing agent, as an MCP client,
// reads every open revenue offer, picks the one with the most counted revenue
// per 1% of price from 2+ counted payers or NEAR AI-verified revenue, buys the smallest part
// the offer allows (offline: the recorded 10%), and reads its portfolio. No model: a fixed rule, so the
// run costs nothing and is repeatable. Every response is validated against
// schemas/mcp/.
//
// Offline (default): the server replays recorded fixtures; nothing is sent.
// --network testnet --root ROOT.testnet: carol's MCP server signs with her
// key (read from its 0600 key file into the server env only) and the buy is a
// real testnet transaction in xUSD-FAKE. Run after `make scene-12`.
//
// Usage: node dist/scripts/agent-invests.js [--network testnet --root R [--agents a,b]] [--out PATH]
import { writeFileSync, mkdirSync } from "node:fs";
import { dirname } from "node:path";
import { connect, REPO } from "../src/client.js";
import { loadSchemas, type ToolName } from "../src/tools.js";
import { testnetSetup } from "../src/llm-testnet.js";

const arg = (f: string): string | undefined => {
  const i = process.argv.indexOf(f);
  return i >= 0 ? process.argv[i + 1] : undefined;
};

interface Offer { offer_id: number; pct: number; price: string; price_per_pct: string; open_pct: number; min_part_pct: number; seller_is_owner: boolean }
interface AgentOffers { agent: string; counted: string; counted_payers: number; buyable: boolean; offers: Offer[] }

/** The investing rule: counted revenue per dollar of a 1% part, among offers anyone can buy now,
 *  from agents with 2+ counted payers or NEAR AI-verified revenue (`verified`: agent -> dollars). */
export function pick(agents: AgentOffers[], verified: Record<string, string> = {}): { agent: string; offer: Offer; score: number } | null {
  let best: { agent: string; offer: Offer; score: number } | null = null;
  for (const a of agents) {
    if (!a.buyable || (a.counted_payers < 2 && !(Number(verified[a.agent] ?? 0) > 0))) continue;
    for (const o of a.offers) {
      if (o.open_pct < o.min_part_pct || !o.seller_is_owner) continue;
      const score = Number(a.counted) / Number(o.price_per_pct);
      if (!best || score > best.score) best = { agent: a.agent, offer: o, score };
    }
  }
  return best;
}

async function main(): Promise<number> {
  const schemas = loadSchemas(REPO);
  let env: Record<string, string>;
  let investor: string;
  let network: string;
  let pctOverride = arg("--pct") ? Number(arg("--pct")) : undefined;
  if (arg("--network") === "testnet") {
    const setup = testnetSetup(REPO);
    if ("refused" in setup) { process.stderr.write(`agent-invests: refused: ${setup.refused}\n`); return 2; }
    const agents = arg("--agents") ?? `translator3.${setup.root}`;
    env = { ...setup.serverEnv, AGENT_EXIT_AGENTS: agents, AGENT_EXIT_SESSION_CAP: "50" };
    investor = setup.buyer;
    network = `testnet (${setup.root}); the MCP server signs as ${investor} with its own key`;
  } else {
    investor = "dave.testnet";
    env = { AGENT_EXIT_ALLOW_WRITE: "1", AGENT_EXIT_ACCOUNT_ID: investor, AGENT_EXIT_SESSION_CAP: "200" };
    network = "offline (recorded fixtures); nothing is sent";
    // The fixtures recorded one buy: dave takes 10% of research.testnet.
    pctOverride ??= 10;
  }
  const s = await connect(env);
  const steps: unknown[] = [];
  let allValid = true;
  const step = async (why: string, tool: ToolName, args: Record<string, unknown> = {}) => {
    const r = await s.call(tool, args);
    const valid = r.ok ? schemas.validateOutput[tool](r.body) : schemas.validateError(r.body);
    allValid &&= Boolean(valid);
    steps.push({ why, tool, args, ok: r.ok, schema_valid: Boolean(valid), response: r.body });
    process.stderr.write(`${r.ok ? "ok   " : "error"} ${tool.padEnd(12)} ${why}\n`);
    return r;
  };
  let pass = false;
  let chosen: ReturnType<typeof pick> = null;
  try {
    const l = await step("read every open revenue offer", "list_offers");
    const agents = l.ok ? (l.body as { agents: AgentOffers[] }).agents : [];
    // Check each candidate's earnings by who paid before choosing.
    const verified: Record<string, string> = {};
    for (const a of agents.filter((x) => x.buyable && x.offers.length)) {
      const e = await step(`check ${a.agent}'s earnings by who paid`, "get_earnings", { agent: a.agent });
      if (e.ok) verified[a.agent] = (e.body as { earnings: { verified: string } }).earnings.verified;
    }
    chosen = pick(agents, verified);
    if (!chosen) { process.stderr.write("agent-invests: no offer passes the rule\n"); return 1; }
    const pct = pctOverride ?? chosen.offer.min_part_pct;
    const maxPrice = Number(chosen.offer.price_per_pct) * pct;
    const b = await step(`buy ${pct}% of ${chosen.agent}'s future revenue for at most ${maxPrice.toFixed(2)}`, "buy_share",
      { agent: chosen.agent, offer_id: chosen.offer.offer_id, pct, max_price: maxPrice, confirm: true });
    const p = await step("read the portfolio", "portfolio");
    const held = p.ok && (p.body as { holdings: { agent: string }[] }).holdings.some((h) => h.agent === chosen!.agent);
    pass = b.ok && held && allValid;
  } finally {
    await s.close();
  }
  const out = {
    schema: "agent-invests.v1",
    money: "xUSD-FAKE, a fake test dollar with no value",
    network,
    investor,
    rule: "most counted revenue per 1% of price, from 2+ counted payers or NEAR AI-verified revenue, seller is the owner; buy the smallest part",
    chosen: chosen && { agent: chosen.agent, offer_id: chosen.offer.offer_id, score: Number(chosen.score.toFixed(4)) },
    steps,
    txs: steps.flatMap((x) => {
      const r = (x as { tool: string; ok: boolean; response: { tx_hash?: string; explorer?: string } });
      return r.response?.tx_hash ? [{ tool: r.tool, ok: r.ok, tx_hash: r.response.tx_hash, explorer: r.response.explorer }] : [];
    }),
    all_schema_valid: allValid,
    pass,
  };
  const outPath = arg("--out") ?? "out/agent-invests.json";
  mkdirSync(dirname(outPath), { recursive: true });
  writeFileSync(outPath, JSON.stringify(out, null, 2) + "\n");
  process.stderr.write(`agent-invests: wrote ${outPath}; pass=${pass}\n`);
  return pass ? 0 : 1;
}

if (process.argv[1] && /agent-invests\.js$/.test(process.argv[1])) {
  main().then((c) => process.exit(c), (e) => { process.stderr.write(`agent-invests: ${(e as Error).message}\n`); process.exit(1); });
}
