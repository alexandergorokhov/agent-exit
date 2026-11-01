// The Agent Skill: frontmatter has name and description, and every command,
// path and tool it references exists.
import { test } from "node:test";
import assert from "node:assert/strict";
import { existsSync, readFileSync } from "node:fs";
import { join } from "node:path";
import { TOOL_NAMES } from "../src/tools.js";
import { REPO } from "./helpers.js";

const text = readFileSync(join(REPO, "skills", "agent-exit", "SKILL.md"), "utf8");

test("frontmatter", () => {
  const m = text.match(/^---\n([\s\S]*?)\n---\n/);
  assert.ok(m, "frontmatter block");
  const fields = Object.fromEntries(
    m[1].split("\n").map((l) => [l.slice(0, l.indexOf(":")).trim(), l.slice(l.indexOf(":") + 1).trim()]),
  );
  assert.equal(fields.name, "agent-exit");
  assert.match(fields.name, /^[a-z0-9-]{1,64}$/);
  assert.ok(fields.description.length > 50 && fields.description.length <= 1024);
  assert.match(fields.description, /scan/i);
});

test("every make target, path and tool it names exists", () => {
  const makefile = readFileSync(join(REPO, "Makefile"), "utf8");
  for (const [, target] of text.matchAll(/`make ([a-z0-9-]+)/g)) {
    assert.match(makefile, new RegExp(`^${target}:`, "m"), `make ${target}`);
    assert.doesNotMatch(
      makefile.split(new RegExp(`^${target}:`, "m"))[1].split("\n\n")[0],
      /not_built/,
      `make ${target} must be built`,
    );
  }
  for (const [, p] of text.matchAll(/`((?:mcp|schemas|docs|skills|fixtures)\/[^`\s]*)`/g)) {
    assert.ok(existsSync(join(REPO, p)), p);
  }
  for (const [, tool] of text.matchAll(/`([a-z_]+)`/g)) {
    if (tool.includes("_") && /^(list|get|scan|buy|verify)_/.test(tool)) {
      assert.ok((TOOL_NAMES as readonly string[]).includes(tool), tool);
    }
  }
});

test("the MCP config example parses and points at the built server", () => {
  const cfg = JSON.parse(readFileSync(join(REPO, "mcp", "mcp-config.example.json"), "utf8"));
  const s = cfg.mcpServers["agent-exit"];
  assert.equal(s.command, "node");
  assert.ok(existsSync(join(REPO, s.args[0])), s.args[0]);
  assert.notEqual(s.env.AGENT_EXIT_NETWORK, "mainnet");
});
