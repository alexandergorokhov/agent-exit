// `make translator-serve`: the translator worker on http://127.0.0.1:PORT.
//
//   POST /job  {"payer":"p1.testnet","amount":"12000000","text":"...","target_lang":"French","job_id":"..."}
//   GET  /health
//
// Offline (the default): serves recorded fixture jobs signed with local test
// keys and says so in every answer. Real mode: AGENT_EXIT_NETWORK=testnet,
// NEARAI_API_KEY (env or ~/.agent-exit.env, mode 600) and
// AGENT_EXIT_TRANSLATOR_ACCOUNT (the .testnet account of the translator agent).
// Exit 2 when real mode lacks one of those.
//
// Usage: node dist/scripts/translator-serve.js [--port 8787] [--max-jobs 20] [--check]
//   --check validates the start-up settings and exits without listening.
import { readFileSync } from "node:fs";
import { join } from "node:path";
import { REPO } from "../src/client.js";
import { loadModelEnv, NEARAI_BASE_URL, NEARAI_DEFAULT_MODEL } from "../src/llm.js";
import { FixtureTranslator, NearAiTranslator, startServer, type Translator } from "../src/translator.js";

const arg = (f: string): string | undefined => {
  const i = process.argv.indexOf(f);
  return i >= 0 ? process.argv[i + 1] : undefined;
};
const say = (l: string) => process.stderr.write(`translator-serve: ${l}\n`);

function build(): Translator | number {
  const net = (process.env.AGENT_EXIT_NETWORK || "offline").toLowerCase();
  if (net !== "offline" && net !== "testnet") {
    say(`AGENT_EXIT_NETWORK=${net} refused: only offline (default) or testnet`);
    return 2;
  }
  if (net === "offline") {
    const fixture = JSON.parse(readFileSync(join(REPO, "fixtures", "test-signatures", "translator.json"), "utf8"));
    return new FixtureTranslator(fixture);
  }
  let env: Record<string, string>;
  try {
    env = loadModelEnv();
  } catch (e) {
    say(`refused: ${(e as Error).message}`);
    return 2;
  }
  if (!env.NEARAI_API_KEY) {
    say("needs NEARAI_API_KEY for testnet mode (put it in ~/.agent-exit.env, mode 600). Nothing was started.");
    return 2;
  }
  const agent = process.env.AGENT_EXIT_TRANSLATOR_ACCOUNT?.trim();
  if (!agent) {
    say("needs AGENT_EXIT_TRANSLATOR_ACCOUNT: the .testnet account of the translator agent the payments go to");
    return 2;
  }
  try {
    return new NearAiTranslator({
      apiKey: env.NEARAI_API_KEY,
      baseUrl: env.NEARAI_BASE_URL || NEARAI_BASE_URL,
      model: env.NEARAI_MODEL || NEARAI_DEFAULT_MODEL,
      agent,
    });
  } catch (e) {
    say(`refused: ${(e as Error).message}`);
    return 2;
  }
}

async function main(): Promise<number> {
  const tr = build();
  if (typeof tr === "number") return tr;
  say(
    tr.mode === "offline"
      ? `mode offline: serving a recorded fixture signed with local test keys, not NEAR AI (${tr.label})`
      : `mode testnet: ${tr.label}`,
  );
  if (process.argv.includes("--check")) return 0;
  const port = Number(arg("--port") ?? "8787");
  const maxJobs = Number(arg("--max-jobs") ?? "20");
  if (!Number.isInteger(port) || port < 0 || port > 65535 || !Number.isInteger(maxJobs) || maxJobs < 1) {
    say("--port and --max-jobs must be whole numbers");
    return 2;
  }
  const server = await startServer(tr, { port, maxJobs });
  const addr = server.address();
  say(`listening on http://127.0.0.1:${typeof addr === "object" && addr ? addr.port : port} (POST /job, GET /health); at most ${maxJobs} jobs`);
  await new Promise(() => {}); // until killed
  return 0;
}

main().then(
  (c) => process.exit(c),
  (e) => {
    say((e as Error).message);
    process.exit(1);
  },
);
