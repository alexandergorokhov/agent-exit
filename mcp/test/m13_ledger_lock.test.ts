// M13 (review 5, L1): the spend ledger's lock file. A process removes only
// its own lock, so a slow process cannot delete a lock another process holds
// (which would let two processes pass the cap at once); a crashed process's
// stale lock is still taken over.
import { test } from "node:test";
import assert from "node:assert/strict";
import { existsSync, mkdtempSync, readFileSync, utimesSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { SpendLedger } from "../src/tools.js";

const dir = () => mkdtempSync(join(tmpdir(), "agent-exit-m13-"));

test("M13 a process never unlinks a lock that is no longer its own", () => {
  const file = join(dir(), "spend.json");
  const lock = `${file}.lock`;
  const l = new SpendLedger(file, "a.testnet");
  // Simulate: while we hold the lock, it is judged stale and taken by another process.
  const orig = (l as unknown as { write(d: bigint): void }).write.bind(l);
  (l as unknown as { write(d: bigint): void }).write = (d: bigint) => {
    orig(d);
    writeFileSync(lock, "someone-else");
  };
  l.add(5n);
  assert.ok(existsSync(lock), "the other process's lock is left in place");
  assert.equal(readFileSync(lock, "utf8"), "someone-else");
});

test("M13 a stale lock left by a crashed process is taken over", () => {
  const file = join(dir(), "spend.json");
  const lock = `${file}.lock`;
  writeFileSync(lock, "crashed");
  const old = (Date.now() - 60_000) / 1000;
  utimesSync(lock, old, old);
  const l = new SpendLedger(file, "a.testnet");
  l.reserve(10n, 100n);
  assert.equal(l.total(), 10n);
  assert.equal(existsSync(lock), false, "our own lock is removed afterwards");
});
