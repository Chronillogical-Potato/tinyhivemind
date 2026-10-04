import assert from "node:assert/strict";
import { test } from "node:test";
import { MARK_EVENTS, byLane, markLane, markTitle, memoryFailed } from "./marks.js";

const mark = (label, detail = "") => ({ event: "mark", label, detail });
const resumed = { event: "session_resumed", seat: "lead", messages: 9, delta_rows: 2 };
const recalled = { event: "recalled", seat: "lead", moment: "rejoin", notes: 3, chars: 420, latency_ms: 12 };
const remembered = { event: "remembered", seat: "tester", entries: 4, latency_ms: 1400 };

test("typed session and memory events get their own lanes", () => {
  assert.equal(markLane(resumed), "session");
  assert.equal(markLane(recalled), "memory");
  assert.equal(markLane(remembered), "memory");
  for (const e of [resumed, recalled, remembered]) assert.ok(MARK_EVENTS.has(e.event));
  assert.equal(markLane(mark("exec", "ls")), "marks");
  assert.equal(markLane({ event: "checkpoint", label: "digest-1" }), "marks");
});

test("older traces with session and memory marks still land in those lanes", () => {
  assert.equal(markLane(mark("session", "lead: activation 2 messages 9")), "session");
  assert.equal(markLane(mark("memory", "lead: recall rejoin chars=10")), "memory");
});

test("a memory mark with an error is flagged, typed events are not", () => {
  assert.equal(memoryFailed(mark("memory", "lead: recall session_start error=memory recall failed: timed out")), true);
  assert.equal(memoryFailed(mark("memory", "run: belief_build latency_ms=900")), false);
  assert.equal(memoryFailed(recalled), false);
  assert.equal(memoryFailed(mark("exec", "x error=y")), false);
});

test("titles read the typed fields", () => {
  assert.equal(markTitle(resumed), "lead resumed its session: 9 messages, 2 new desk rows");
  assert.match(markTitle(recalled), /recalled \(rejoin\): 3 notes, 420 chars/);
  assert.match(markTitle(remembered), /remembered 4 entries in 1400 ms/);
  assert.equal(markTitle(mark("exec", "ls")), "mark: exec ls");
});

test("events are grouped by lane", () => {
  const lanes = byLane([resumed, recalled, remembered, mark("memory"), mark("context")]);
  assert.deepEqual([lanes.session.length, lanes.memory.length, lanes.marks.length], [1, 3, 1]);
});
