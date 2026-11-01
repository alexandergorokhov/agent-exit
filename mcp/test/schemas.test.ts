// Every tool has v1 input and output schemas that name their schema_version,
// tools/list serves them verbatim, and the golden run validates.
import { test } from "node:test";
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { join } from "node:path";
import { connect } from "../src/client.js";
import { TOOL_NAMES } from "../src/tools.js";
import { REPO, schemas } from "./helpers.js";

test("schema files", () => {
  for (const t of TOOL_NAMES) {
    assert.equal(schemas.output[t].properties.schema_version.const, `agent-exit.mcp.${t}.v1`);
    assert.equal(schemas.input[t].type, "object");
    assert.equal(schemas.input[t].additionalProperties, false);
  }
  assert.equal(schemas.error.properties.schema_version.const, "agent-exit.mcp.error.v1");
});

test("tools/list serves the schema files", async () => {
  const s = await connect({});
  try {
    const { tools } = await s.client.listTools();
    assert.deepEqual(tools.map((t) => t.name), [...TOOL_NAMES]);
    for (const t of tools) {
      const { $schema: _a, $id: _b, ...want } = schemas.input[t.name as (typeof TOOL_NAMES)[number]];
      assert.deepEqual(t.inputSchema, want);
    }
  } finally {
    await s.close();
  }
});

test("golden agent-buys-agent run validates step by step", () => {
  const g = JSON.parse(readFileSync(join(REPO, "tests", "golden", "agent-buys-agent.json"), "utf8"));
  assert.equal(g.schema, "agent-buys-agent.v1");
  assert.equal(g.pass, true);
  for (const s of g.steps) {
    const v = s.ok ? schemas.validateOutput[s.tool as (typeof TOOL_NAMES)[number]] : schemas.validateError;
    assert.ok(v(s.response), `${s.tool}: ${JSON.stringify(v.errors)}`);
  }
});
