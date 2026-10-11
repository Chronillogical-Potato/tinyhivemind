import assert from "node:assert/strict";
import fs from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";
import { test } from "node:test";
import { atEnd, clampStep, crossings, sawBy, stateAt, stepBy, timeline } from "./simulation.js";
import { sseParser } from "./stream.js";

const fixtures = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "../../public/fixtures/hives");
const frames = (name) => fs.readFileSync(path.join(fixtures, name), "utf8").split("\n").filter(Boolean).map((l) => JSON.parse(l));
const board = () => timeline(frames("review-board.frames.ndjson"));

test("groups frames into start, rounds and end", () => {
  const tl = board();
  assert.equal(tl.start.package, "review-board");
  assert.equal(tl.rounds.length, 3);
  assert.equal(tl.end.ending, "converged");
  assert.deepEqual(timeline([{ type: "mystery" }, null]).rounds, []);
});

test("seeks to any step and clamps outside the episode", () => {
  const tl = board();
  assert.equal(clampStep(tl, -4), 0);
  assert.equal(clampStep(tl, 99), 3);
  assert.equal(clampStep(tl, NaN), 0);
  const s = stateAt(tl, 2);
  assert.equal(s.step, 2);
  assert.equal(s.round.round, 2);
  assert.equal(s.lines.length, 5);
  assert.deepEqual([...s.active.keys()], ["lead", "perf"]);
  assert.equal(stateAt(tl, 0).lines.length, 0);
  assert.equal(stateAt(tl, 0).round, null);
});

test("stepping past the end stays on the last round", () => {
  const tl = board();
  assert.equal(stepBy(tl, 3, 1), 3);
  assert.equal(stepBy(tl, 0, -1), 0);
  assert.equal(stepBy(tl, 1, 1), 2);
});

test("shows the ending only on the last step once the end frame arrived", () => {
  const tl = board();
  assert.equal(stateAt(tl, 2).ending, null);
  assert.equal(stateAt(tl, 3).ending.topic, "ship");
  assert.ok(atEnd(tl, 3));
  const streaming = timeline(frames("review-board.frames.ndjson").filter((f) => f.type !== "end"));
  assert.equal(stateAt(streaming, 3).ending, null);
  assert.equal(atEnd(streaming, 3), false);
});

test("carries standings forward over a round that reports none", () => {
  const tl = board();
  tl.rounds[1].standings = [];
  assert.deepEqual(stateAt(tl, 2).standings, tl.rounds[0].standings);
  delete tl.rounds[1].standings;
  assert.deepEqual(stateAt(tl, 2).standings, tl.rounds[0].standings);
  assert.equal(stateAt(tl, 3).standings.find((s) => s.topic === "ship").committed, true);
});

test("tracks each seat's latest answer and its correctness", () => {
  const tl = timeline(frames("math-swarm.frames.ndjson"));
  const end = stateAt(tl, tl.rounds.length);
  assert.equal(end.start.seats, 50);
  assert.equal(end.problem.answer, "924");
  for (const { answer, correct } of end.answers.values()) assert.equal(correct, answer === "924");
  assert.ok(end.answers.size > 10);
});

test("a blind turn saw only earlier rounds; a full turn saw its own round too", () => {
  const tl = board();
  const s = stateAt(tl, 3);
  const perf = sawBy(s, "perf"); // round 2, full, saw 5 of 5
  assert.equal(perf.turn.round, 2);
  assert.deepEqual([...perf.seen].sort(), [0, 1, 2, 3]);
  const style = sawBy(stateAt(tl, 1), "style"); // round 1, blind
  assert.equal(style.seen.size, 0);
  assert.equal(sawBy(s, "nobody"), null);
});

test("finds desk crossings between consecutive rounds", () => {
  const r = (...desks) => ({ turns: desks.map((desk) => ({ desk })) });
  assert.deepEqual(crossings(r("verify"), r("synthesis")), [{ from: "verify", to: "synthesis" }]);
  assert.deepEqual(crossings(r("a"), r("a")), []);
  assert.deepEqual(crossings(null, r("a")), []);
  assert.deepEqual(crossings(r("a", "b"), r("b", "c")), [{ from: "a", to: "c" }, { from: "b", to: "c" }]);
});

test("parses server-sent events split across chunks", () => {
  const p = sseParser();
  assert.deepEqual(p.push("event: frame\ndata: {\"a\""), []);
  assert.deepEqual(p.push(":1}\n\nevent: done\r\ndata: {}\r\n\r\n: comment\n\n"), [
    { event: "frame", data: '{"a":1}' },
    { event: "done", data: "{}" },
  ]);
  assert.deepEqual(p.push("data: x\ndata: y\n\n"), [{ event: "message", data: "x\ny" }]);
});
