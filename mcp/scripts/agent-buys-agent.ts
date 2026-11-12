// Scripted, offline "agent buys agent" (S14): the `summarizer` agent, as an
// MCP client, finds, checks and buys the `translator` agent through the
// Agent Exit MCP server, then checks the old seller is locked out and lists
// the agent again (S15). Every response is validated against schemas/mcp/.
//
// Offline: the server replays results RECORDED from the real contract code.
// Nothing is sent to any chain. All money is xUSD-FAKE.
//
// Usage: node dist/scripts/agent-buys-agent.js [--out PATH] [--golden PATH]
import { readFileSync, writeFileSync, mkdirSync } from "node:fs";
import { dirname, join } from "node:path";
import { connect, REPO } from "../src/client.js";
import { loadSchemas, type ToolName } from "../src/tools.js";

const arg = (f: string): string | undefined => {
  const i = process.argv.indexOf(f);
  return i >= 0 ? process.argv[i + 1] : undefined;
};

async function main(): Promise<number> {
  const world = JSON.parse(readFileSync(join(REPO, "fixtures", "mcp", "world.json"), "utf8"));
  const buyer = "summarizer.testnet";
  const runnerPk: string = world.recorded_buys[0].buyer_runner_pk;
  const schemas = loadSchemas(REPO);
  const s = await connect({
    AGENT_EXIT_ALLOW_WRITE: "1",
    AGENT_EXIT_ACCOUNT_ID: buyer,
    AGENT_EXIT_SESSION_CAP: "1500",
    // The runner key comes from the server env, never from tool input (A37).
    AGENT_EXIT_RUNNER_PK: runnerPk,
  });
  const steps: unknown[] = [];
  let allValid = true;
  const step = async (why: string, tool: ToolName, args: Record<string, unknown> = {}) => {
    const r = await s.call(tool, args);
    const valid = r.ok ? schemas.validateOutput[tool](r.body) : schemas.validateError(r.body);
    allValid &&= Boolean(valid);
    steps.push({ why, tool, args, ok: r.ok, schema_valid: Boolean(valid), response: r.body });
    process.stderr.write(`${r.ok ? "ok   " : "error"} ${tool.padEnd(15)} ${why}\n`);
    return r;
  };

  const list = await step("find agents for sale", "list_agents", { status: "Listed" });
  await step("cheat claims 500.00 gross: how much is real?", "get_earnings", { agent: "cheat.testnet" });
  await step("scan the cheap one first", "scan_agent", { agent: "cheat.testnet" });
  const refused = await step("try to buy it anyway: refused before any transaction", "buy_agent", {
    agent: "cheat.testnet", max_price: 600, confirm: true,
  });
  const earn = await step("translator: real customers?", "get_earnings", { agent: "translator.factory.testnet" });
  const scan = await step("scan translator", "scan_agent", { agent: "translator.factory.testnet" });
  const before = await step("seller still in control before the sale", "verify_lockout", {
    agent: "translator.factory.testnet", account: "alice.testnet",
  });
  const buy = await step("buy translator inside max_price, confirmed", "buy_agent", {
    agent: "translator.factory.testnet", max_price: 1000, confirm: true,
  });
  const lock = await step("is the old seller locked out?", "verify_lockout", {
    agent: "translator.factory.testnet", account: "alice.testnet",
  });
  const resale = await step("resale: list translator again at 1,200.00", "list_for_sale", {
    agent: "translator.factory.testnet", price: 1200, confirm: true,
  });
  await s.close();

  const summary = {
    agents_listed: list.body.agents?.length ?? 0,
    cheat_buy_refused: !refused.ok && refused.body.error.code === "SCAN_BLOCKED",
    translator_counted: earn.body.earnings?.counted,
    translator_verified: earn.body.earnings?.verified,
    translator_scan: scan.body.verdict,
    seller_locked_out_before: before.body.locked_out,
    bought: buy.ok,
    price: buy.body.price,
    paid_now: buy.body.paid_now,
    escrow: buy.body.escrow,
    owner_after: buy.body.owner_after,
    seller_locked_out_after: lock.body.locked_out,
    relisted_at: resale.body.price,
    all_responses_schema_valid: allValid,
  };
  const ok =
    summary.cheat_buy_refused && summary.bought && summary.seller_locked_out_after === true &&
    summary.seller_locked_out_before === false && allValid;
  const out = {
    schema: "agent-buys-agent.v1",
    money: "xUSD-FAKE, a fake test dollar with no value",
    network: "offline (results recorded from the real contract code; nothing sent)",
    buyer,
    steps,
    summary,
    pass: ok,
  };
  const text = JSON.stringify(out, null, 2) + "\n";
  const outPath = arg("--out");
  if (outPath) {
    mkdirSync(dirname(outPath), { recursive: true });
    writeFileSync(outPath, text);
    process.stderr.write(`agent-buys-agent: wrote ${outPath}\n`);
  } else {
    process.stdout.write(text);
  }
  process.stderr.write(`agent-buys-agent: ${JSON.stringify(summary)}\n`);
  const golden = arg("--golden");
  if (golden) {
    const want = readFileSync(golden, "utf8");
    if (want !== text) {
      process.stderr.write(`agent-buys-agent: output differs from ${golden}\n`);
      return 1;
    }
    process.stderr.write(`agent-buys-agent: output equals ${golden}\n`);
  }
  return ok ? 0 : 1;
}

main().then((c) => process.exit(c), (e) => {
  process.stderr.write(`${(e as Error).stack}\n`);
  process.exit(1);
});
