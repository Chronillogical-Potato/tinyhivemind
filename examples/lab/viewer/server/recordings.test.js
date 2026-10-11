// Unit tests for recordings.js and recordings-api.js: scanning run
// directories, id safety, live detection, the main-checkout resolution, and
// tailing a frames file that is still being written (partial lines included).
import assert from "node:assert/strict";
import fs from "node:fs";
import { EventEmitter } from "node:events";
import os from "node:os";
import path from "node:path";
import { test } from "node:test";
import { recordingsApi } from "./recordings-api.js";
import { LIVE_MS, mainCheckout, recordingRoots, resolveRecording, scanRecordings, tailFrames } from "./recordings.js";

const tmp = () => fs.mkdtempSync(path.join(os.tmpdir(), "hive-rec-"));
const line = (o) => `${JSON.stringify(o)}\n`;
const START = line({ type: "start", package: "coin-panel", seed: 1, seats: 4, problem: { id: "coins-10", statement: "s", answer: "10" } });
const ROUND = line({ type: "round", round: 1, desk: "solve", turns: [{ seat: "a", tool_calls: [{ tool: "run_python", ok: true }] }, { seat: "b" }] });
const END = line({ type: "end", ending: "converged", topic: "10", correct: true, rounds: 1, turns: 0 });

function run(root, rel, body, { structure = true, ageMs = 0 } = {}) {
  const dir = path.join(root, rel);
  fs.mkdirSync(dir, { recursive: true });
  const file = path.join(dir, "frames.ndjson");
  fs.writeFileSync(file, body);
  if (structure) fs.writeFileSync(path.join(dir, "structure.json"), "{}");
  if (ageMs) {
    const t = new Date(Date.now() - ageMs);
    fs.utimesSync(file, t, t);
  }
  return file;
}

const waitUntil = async (cond, ms = 3000) => {
  const until = Date.now() + ms;
  while (!cond()) {
    if (Date.now() > until) throw new Error("timed out");
    await new Promise((r) => setTimeout(r, 10));
  }
};

test("scan finds frames.ndjson at any depth up to five and describes the run", () => {
  const root = tmp();
  run(root, "host/pe/run-1790000000000", START + ROUND + END, { ageMs: 5000 });
  run(root, "a/b/c/d/e/f/too-deep", START);
  fs.writeFileSync(path.join(root, "frames.txt"), START);
  const [rec, ...rest] = scanRecordings([root]);
  assert.equal(rest.length, 0);
  assert.equal(rec.id, "0:host/pe/run-1790000000000");
  assert.equal(rec.package, "coin-panel");
  assert.deepEqual(rec.problem, { id: "coins-10", statement: "s" });
  assert.equal(rec.frames, 3);
  assert.equal(rec.seed, 1);
  assert.equal(rec.round, 1);
  assert.equal(rec.desk, "solve");
  assert.equal(rec.turns, 2);
  assert.equal(rec.tool_calls, 1);
  assert.equal(rec.calls, 2);
  assert.equal(rec.started, 1790000000000);
  assert.equal(rec.ended, true);
  assert.equal(rec.ending, "converged");
  assert.equal(rec.live, false);
  assert.equal(rec.structure, true);
});

test("a recent run without an end frame is live; an old one or an ended one is not", () => {
  const root = tmp();
  run(root, "fresh", START + ROUND);
  run(root, "stale", START + ROUND, { ageMs: LIVE_MS + 5000 });
  run(root, "done", START + END);
  const by = Object.fromEntries(scanRecordings([root]).map((r) => [r.path, r]));
  assert.equal(by.fresh.live, true);
  assert.equal(by.fresh.active, true);
  assert.equal(by.stale.live, false);
  assert.equal(by.stale.active, true);
  assert.equal(by.done.live, false);
});

test("a partial last line is not counted as a frame", () => {
  const root = tmp();
  run(root, "r", START + ROUND + '{"type":"rou');
  assert.equal(scanRecordings([root])[0].frames, 2);
});

test("ids resolve only to frames.ndjson inside a root", () => {
  const root = tmp();
  const outside = tmp();
  run(root, "ok", START);
  run(outside, "secret", START);
  fs.symlinkSync(path.join(outside, "secret"), path.join(root, "link"));
  fs.writeFileSync(path.join(root, "loose.ndjson"), START);
  assert.equal(resolveRecording([root], "0:ok").frames, path.join(root, "ok", "frames.ndjson"));
  assert.equal(resolveRecording([root], "0:ok").structure, path.join(root, "ok", "structure.json"));
  for (const bad of ["0:../" + path.basename(outside) + "/secret", "0:link", "1:ok", "ok", "0:", "0:missing", "0:ok/../../x", null, "0:loose.ndjson"]) {
    assert.equal(resolveRecording([root], bad), null, String(bad));
  }
});

test("roots come from HIVE_RECORDINGS, colon-separated, keeping only directories", () => {
  const a = tmp(), b = tmp();
  assert.deepEqual(recordingRoots({ HIVE_RECORDINGS: `${a}:${b}:/no/such/dir` }, []), [a, b]);
  assert.deepEqual(recordingRoots({}, [b]), [b]);
});

test("mainCheckout follows a worktree's gitdir pointer through commondir", () => {
  const main = fs.realpathSync(tmp());
  const gitdir = path.join(main, ".git", "worktrees", "wt");
  fs.mkdirSync(gitdir, { recursive: true });
  fs.writeFileSync(path.join(gitdir, "commondir"), "../..\n");
  const wt = path.join(main, "worktrees", "wt");
  fs.mkdirSync(wt, { recursive: true });
  fs.writeFileSync(path.join(wt, ".git"), `gitdir: ${gitdir}\n`);
  assert.equal(mainCheckout(wt), main);
  assert.equal(mainCheckout(main), main); // .git is a directory
  const sub = tmp();
  fs.writeFileSync(path.join(sub, ".git"), "gitdir: ../modules/x\n"); // a submodule: no commondir
  assert.equal(mainCheckout(sub), sub);
});

test("tail sends existing lines, then appended ones, joining a line split across writes", async () => {
  const root = tmp();
  const file = run(root, "r", START);
  const lines = [];
  let ended = null;
  const t = tailFrames(file, { intervalMs: 10, onLine: (l) => lines.push(l), onEnd: (e) => { ended = e; } });
  await waitUntil(() => lines.length === 1);
  fs.appendFileSync(file, ROUND.slice(0, 7));
  await new Promise((r) => setTimeout(r, 40));
  assert.equal(lines.length, 1, "a partial line waits for its newline");
  fs.appendFileSync(file, ROUND.slice(7) + END);
  await waitUntil(() => ended);
  assert.deepEqual(ended, { ended: true });
  assert.deepEqual(lines.map((l) => JSON.parse(l).type), ["start", "round", "end"]);
  t.stop();
});

test("tail splits a multi-byte character across reads without mangling it", async () => {
  const root = tmp();
  const file = run(root, "r", "");
  const lines = [];
  const t = tailFrames(file, { intervalMs: 10, onLine: (l) => lines.push(l) });
  const bytes = Buffer.from(line({ type: "round", content: "naïve ≥ 10" }));
  const cut = bytes.indexOf(0xc3) + 1; // inside the two-byte "ï"
  fs.appendFileSync(file, bytes.subarray(0, cut));
  await new Promise((r) => setTimeout(r, 40));
  fs.appendFileSync(file, bytes.subarray(cut));
  await waitUntil(() => lines.length === 1);
  t.stop();
  assert.equal(JSON.parse(lines[0]).content, "naïve ≥ 10");
});

test("tail without follow stops after the content there, flushing a final unterminated line", async () => {
  const root = tmp();
  const file = run(root, "r", START + ROUND.trimEnd());
  const lines = [];
  let ended = null;
  tailFrames(file, { follow: false, onLine: (l) => lines.push(l), onEnd: (e) => { ended = e; } });
  await waitUntil(() => ended);
  assert.deepEqual(ended, { ended: false });
  assert.equal(lines.length, 2);
});

test("tail gives up on a file that stopped growing", async () => {
  const root = tmp();
  const file = run(root, "r", START);
  let clock = 0;
  let ended = null;
  tailFrames(file, { intervalMs: 5, staleMs: 100, now: () => clock, onLine: () => {}, onEnd: (e) => { ended = e; } });
  await new Promise((r) => setTimeout(r, 30));
  assert.equal(ended, null);
  clock = 1000;
  await waitUntil(() => ended);
  assert.deepEqual(ended, { ended: false });
});

// A minimal response double: collects writes, emits close on demand.
function response() {
  const res = new EventEmitter();
  res.body = "";
  res.headers = {};
  res.setHeader = (k, v) => { res.headers[k] = v; };
  res.writeHead = (status, headers) => { res.statusCode = status; Object.assign(res.headers, headers); };
  res.write = (chunk) => { res.body += chunk; };
  res.end = (chunk = "") => { res.body += chunk; res.ended = true; res.emit("close"); };
  return res;
}

test("the api lists recordings, serves structure (or a package's), and streams frames to the end", async () => {
  const root = tmp();
  run(root, "own", START + END);
  run(root, "view", START.replace("coin-panel", "math-swarm") + END, { structure: false });
  run(root, "orphan", START.replace("coin-panel", "gone") + END, { structure: false });
  const api = recordingsApi({ roots: [root], intervalMs: 10, hasPackage: (n) => n === "math-swarm",
    packageStructure: async (n) => (n === "math-swarm" ? { id: "math-swarm" } : null) });
  let res = response();
  await api.handle("/recordings", {}, res);
  assert.deepEqual(JSON.parse(res.body).recordings.map((r) => r.path).sort(), ["own", "view"]);
  res = response();
  await api.handle(`/recordings/${encodeURIComponent("0:view")}/structure`, {}, res);
  assert.deepEqual(JSON.parse(res.body), { id: "math-swarm" });
  res = response();
  await api.handle("/recordings/0%3Aorphan/structure", {}, res);
  assert.equal(res.statusCode, 404);
  res = response();
  await api.handle("/recordings/0%3A..%2F..%2Fetc/frames", {}, res);
  assert.equal(res.statusCode, 404);
  res = response();
  await api.handle("/recordings/0%3Aown/frames", {}, res);
  await waitUntil(() => res.ended);
  assert.match(res.body, /^event: status\n/);
  assert.equal((res.body.match(/event: frame/g) ?? []).length, 2);
  assert.match(res.body, /event: done\ndata: \{"ended":true\}/);
  assert.equal(await api.handle("/recordings/0%3Aown", {}, response()), false);
});

test("the events stream sends the listing at once and again when a recording appears or grows", async (t) => {
  const root = tmp();
  const file = run(root, "a", START);
  const api = recordingsApi({ roots: [root], watchMs: 10 });
  const res = response();
  t.after(() => { res.emit("close"); api.close(); });
  await api.handle("/recordings/events", {}, res);
  const events = () => res.body.split("\n\n").filter((b) => b.startsWith("event: recordings"));
  assert.equal(events().length, 1);
  assert.match(events()[0], /"path":"a"/);
  await new Promise((r) => setTimeout(r, 40));
  assert.equal(events().length, 1, "nothing changed, nothing sent");
  run(root, "b", START);
  await waitUntil(() => events().length === 2);
  assert.match(events()[1], /"path":"b"/);
  fs.appendFileSync(file, ROUND);
  await waitUntil(() => events().length === 3);
});
