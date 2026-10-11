// npm test: the pure half of recordings — turn activity (tool calls and
// memory, counts or lists), per-seat sums, live/finished split, new-run
// detection and labels.
import assert from "node:assert/strict";
import { test } from "node:test";
import { elapsed, finished, liveNow, newlyLive, recordingLabel, seatActivity, toolIcon, turnActivity } from "./activity.js";

test("turn activity reads tool calls and memory as counts or lists", () => {
  const a = turnActivity({ tool_calls: [{ tool: "run_python", summary: "x", ok: false }, { tool: "web_search", ok: true }, { tool: "grep" }],
    memory: { recalled: 3, remembered: ["m1", "m2"] } });
  assert.deepEqual(a.tools.map((t) => [t.icon, t.ok]), [["🐍", false], ["🔎", true], ["🔧", null]]);
  assert.equal(a.recalled, 3);
  assert.equal(a.remembered, 2);
  assert.deepEqual(turnActivity({ content: "plain" }), { tools: [], recalled: 0, remembered: 0 });
  assert.deepEqual(turnActivity({ memory: { recalled: -2, remembered: "x" } }), { tools: [], recalled: 0, remembered: 0 });
  assert.equal(toolIcon("python3"), "🐍");
});

test("seat activity sums every turn of a seat and skips quiet seats", () => {
  const m = seatActivity([
    { seat: "a", tool_calls: [{ tool: "run_python", ok: false }], memory: { recalled: 1 } },
    { seat: "a", tool_calls: [{ tool: "run_python", ok: true }], memory: { remembered: 2 } },
    { seat: "b", content: "nothing" },
  ]);
  assert.deepEqual(m.get("a"), { tools: 2, failed: 1, recalled: 1, remembered: 2 });
  assert.equal(m.has("b"), false);
});

test("live and finished split the listing, and only new live runs are announced", () => {
  const list = [{ id: "a", live: true }, { id: "b", active: true }, { id: "c", ended: true }, { id: "d" }];
  assert.deepEqual(liveNow(list).map((r) => r.id), ["a", "b"]);
  assert.deepEqual(finished(list).map((r) => r.id), ["c", "d"]);
  assert.deepEqual(newlyLive(null, list), [], "the first listing announces nothing");
  assert.deepEqual(newlyLive([{ id: "a" }], [...list, { id: "e", live: true }]).map((r) => r.id), ["b", "e"]);
});

test("labels and elapsed times read naturally", () => {
  assert.equal(recordingLabel({ package: "math-swarm", seed: 4, round: 7, desk: "verify", calls: 31 }), "math-swarm · seed 4 · round 7 on verify · 31 calls");
  assert.equal(recordingLabel({ package: "p", problem: { id: "coins-10" }, turns: 0 }), "p · problem coins-10 · not started · 0 calls");
  assert.equal(elapsed(45_000), "45 s");
  assert.equal(elapsed(3 * 60_000 + 10_000), "3 min");
  assert.equal(elapsed(125 * 60_000), "2 h 05 min");
  assert.equal(elapsed(-1), "?");
});
