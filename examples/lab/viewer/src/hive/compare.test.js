import assert from "node:assert/strict";
import fs from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";
import { test } from "node:test";
import { order, outcome, summarize } from "./compare.js";

const file = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "../../public/fixtures/hives/compare.ndjson");
const events = fs.readFileSync(file, "utf8").split("\n").filter(Boolean).map((l) => JSON.parse(l));

test("summarizing the streamed seeds matches the closing summary", () => {
  const seeds = events.filter((e) => e.type === "seed");
  const summary = events.find((e) => e.type === "summary");
  const mine = summarize(seeds);
  for (const row of summary.rows) {
    const m = mine.find((r) => r.package === row.package);
    assert.ok(Math.abs(m.accuracy - row.accuracy) < 0.01, row.package);
    assert.ok(Math.abs(m.converged - row.converged) < 0.01, row.package);
    assert.ok(Math.abs(m.mean_rounds - row.mean_rounds) < 0.01, row.package);
  }
});

test("classifies a result as right, wrong or no decision", () => {
  assert.equal(outcome({ ending: "converged", correct: true }), "right");
  assert.equal(outcome({ ending: "converged", correct: false }), "wrong");
  assert.equal(outcome({ ending: "deadlocked", correct: null }), "none");
  assert.equal(outcome({ ending: "converged", correct: null }), "none");
});

test("puts the single-seat baseline first, then the requested order", () => {
  const rows = [{ package: "math-swarm" }, { package: "single-seat" }, { package: "math-panel" }];
  assert.deepEqual(order(rows, ["math-panel", "math-swarm"]).map((r) => r.package), ["single-seat", "math-panel", "math-swarm"]);
});
