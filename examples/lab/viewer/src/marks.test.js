import assert from "node:assert/strict";
import { test } from "node:test";
import { byLane, markLane, memoryFailed } from "./marks.js";

const mark = (label, detail = "") => ({ event: "mark", label, detail });

test("session and memory marks get their own lanes", () => {
  assert.equal(markLane(mark("session", "lead: activation 2 messages 9")), "session");
  assert.equal(markLane(mark("memory", "lead: recall rejoin chars=10")), "memory");
  assert.equal(markLane(mark("exec", "ls")), "marks");
  assert.equal(markLane({ event: "checkpoint", label: "digest-1" }), "marks");
});

test("a memory mark with an error is flagged", () => {
  assert.equal(memoryFailed(mark("memory", "lead: recall session_start chars=0 error=timed out")), true);
  assert.equal(memoryFailed(mark("memory", "lead: remember activation chars=40")), false);
  assert.equal(memoryFailed(mark("exec", "x error=y")), false);
});

test("events are grouped by lane", () => {
  const lanes = byLane([mark("session"), mark("memory"), mark("memory"), mark("context")]);
  assert.deepEqual([lanes.session.length, lanes.memory.length, lanes.marks.length], [1, 2, 1]);
});
