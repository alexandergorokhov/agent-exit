// Short human summary for `make demo`, read from the JSON the demo wrote to out/.
import { readFileSync } from "node:fs";

const out = process.argv[2] || "out";
const j = (f) => JSON.parse(readFileSync(`${out}/${f}`, "utf8"));
const v = j("verify.json");
const a = j("attack.json");
const b = j("agent-buys-agent.json").summary;
const t = v.world.translator;

console.log(`  contract checks replayed: ${v.summary.passed} passed, ${v.summary.failed} failed (equal to the golden)`);
console.log(`  translator earned ${t.gross}: ${t.counted} counted, ${t.verified} verified, ${t.flagged} flagged (self-payments)`);
console.log(`  attack suite: ${a.summary.text}`);
console.log(`  scripted buyer: refused the cheat agent = ${b.cheat_buy_refused}; bought translator for ${b.price} (${b.paid_now} to the seller now, ${b.escrow} escrow)`);
console.log(`  after the buy: owner is ${b.owner_after}; old seller locked out = ${b.seller_locked_out_after}`);
console.log(`  full JSON: ${out}/verify.json, ${out}/attack.json, ${out}/agent-buys-agent.json`);
