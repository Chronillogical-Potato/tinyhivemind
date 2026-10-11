// Execute the page coordinator's real run/finally and ownership classifier
// with only its UI/stream boundaries stubbed; no browser or paid calls.
import assert from "node:assert/strict";
import fs from "node:fs";
import vm from "node:vm";
import { test } from "node:test";

const source = fs.readFileSync(new URL("./index.js", import.meta.url), "utf8");
const run = source.slice(source.indexOf("async function run()"), source.indexOf("\nfunction filterDesk"));
const ownRun = source.match(/^const ownRun = .*;$/m)[0];

function page(stream) {
  const ctx = {
    current: { name: "math-panel", recording: null }, runAbort: null, selected: null,
    runStartedAt: 0, AbortController, Date: { now: () => 1000 },
    writeUrl() {}, controls: { liveKnobs: () => ({}), knobs: () => ({}), status() {} },
    player: { reset() {}, finish() { ctx.finished++; }, state: () => ({ received: 0 }) },
    runHive: stream, finished: 0,
  };
  vm.createContext(ctx);
  vm.runInContext(`${run}\n${ownRun}\nglobalThis.start = run; globalThis.owns = ownRun;`, ctx);
  return ctx;
}

test("a completed page run releases ownership of later external recordings", async () => {
  const ctx = page(async () => {});
  await ctx.start();
  assert.equal(ctx.finished, 1);
  assert.equal(ctx.runAbort, null, "the completed controller is released");
  assert.equal(ctx.owns({ package: "math-panel", started: 2000 }), false);
});

test("an older run completing cannot clear a newer run's controller", async () => {
  let release;
  let calls = 0;
  const ctx = page(() => ++calls === 1 ? new Promise((resolve) => { release = resolve; }) : new Promise(() => {}));
  const old = ctx.start();
  ctx.start();
  const current = ctx.runAbort;
  release();
  await old;
  assert.equal(ctx.runAbort, current);
  assert.equal(ctx.finished, 0);
  assert.equal(ctx.owns({ package: "math-panel", started: 2000 }), true);
});
