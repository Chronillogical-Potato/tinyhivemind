import assert from "node:assert/strict";
import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import { test } from "node:test";
import { resolveId, scan } from "./scan.js";

function trial(root, job, trialName, mode, reward) {
  const dir = path.join(root, job, trialName, "agent");
  fs.mkdirSync(dir, { recursive: true });
  fs.writeFileSync(path.join(dir, "trace.jsonl"), '{"run":"r","seq":0,"at_ms":0,"event":"idle"}\n');
  fs.writeFileSync(path.join(dir, "result.json"), JSON.stringify({ mode, tokens_in: 10, tokens_out: 5, wall_ms: 2000, turns: 3 }));
  fs.writeFileSync(path.join(root, job, trialName, "result.json"), JSON.stringify({ verifier_result: { rewards: { reward } } }));
}

function tmp() {
  return fs.mkdtempSync(path.join(os.tmpdir(), "hive-scan-"));
}

test("describes a harbor trial with its mode, reward and tag", () => {
  const root = tmp();
  trial(root, "r1-hive", "fix-git__abc", "hive", 1);
  const [run] = scan([root]);
  assert.equal(run.task, "fix-git");
  assert.equal(run.mode, "hive");
  assert.equal(run.tag, "r1");
  assert.equal(run.reward, 1);
  assert.equal(run.tokens, 15);
  assert.equal(run.turns, 3);
});

test("pairs arms of one task under one tag", () => {
  const root = tmp();
  trial(root, "r1-single", "fix-git__a", "single", 0);
  trial(root, "r1-hive", "fix-git__b", "hive", 1);
  const runs = scan([root]);
  assert.equal(new Set(runs.map((r) => `${r.tag}/${r.task}`)).size, 1);
  assert.deepEqual(runs.map((r) => r.mode).sort(), ["hive", "single"]);
});

test("lists a bare jsonl file named by its path", () => {
  const root = tmp();
  fs.writeFileSync(path.join(root, "sample.jsonl"), "");
  const [run] = scan([root]);
  assert.equal(run.task, "sample");
  assert.equal(run.mode, null);
  assert.equal(run.reward, null);
});

test("resolves ids inside a root and refuses escapes", () => {
  const root = tmp();
  fs.writeFileSync(path.join(root, "a.jsonl"), "");
  fs.writeFileSync(path.join(path.dirname(root), "outside.jsonl"), "");
  assert.ok(resolveId([root], "0:a.jsonl"));
  assert.equal(resolveId([root], "0:../outside.jsonl"), null);
  assert.equal(resolveId([root], "1:a.jsonl"), null);
  assert.equal(resolveId([root], "0:a.txt"), null);
  assert.equal(resolveId([root], "nonsense"), null);
});
