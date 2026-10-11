import assert from "node:assert/strict";
import fs from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";
import { test } from "node:test";
import { multiDeskFrames } from "../../tools/fake-frames.mjs";
import { deskCounts, deskPolicy, deskTabs, describeReferral, edgeFor, entryOnDesk, reasonText } from "./referrals.js";
import { createPlayer } from "./player.js";
import { atEnd, clampStep, crossings, sawBy, stateAt, stepBy, timeline } from "./simulation.js";
import { sseParser } from "./stream.js";

const fixtures = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "../../public/fixtures/hives");
const frames = (name) => fs.readFileSync(path.join(fixtures, name), "utf8").split("\n").filter(Boolean).map((l) => JSON.parse(l));
const board = () => timeline(frames("review-board.frames.ndjson"));
const structure = (name) => JSON.parse(fs.readFileSync(path.join(fixtures, `${name}.json`), "utf8"));
const swarmFrames = (opts) => multiDeskFrames(frames("math-swarm.frames.ndjson"), structure("math-swarm"), opts);
const swarm = (opts) => timeline(swarmFrames(opts));

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

test("clears carried standings when the next desk's first round reports none", () => {
  const round = (n, desk, standings) => ({ type: "round", round: n, desk, desk_round: 1, turns: [], standings });
  const standing = [{ topic: "alpha", supporters: ["a"], objectors: [], committed: false }];
  const tl = timeline([{ type: "start", package: "p" }, round(1, "first", standing), round(2, "second", []), round(3, "second", [])]);
  assert.deepEqual(stateAt(tl, 1).standings, standing);
  assert.deepEqual(stateAt(tl, 2).standings, [], "the second desk has no candidates yet");
  assert.deepEqual(stateAt(tl, 3).standings, []);
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

test("a per-desk stream keeps rounds global and tags each line with its floor", () => {
  const tl = swarm();
  assert.deepEqual(tl.rounds.map((r) => r.round), tl.rounds.map((_, i) => i + 1));
  const s = stateAt(tl, tl.rounds.length);
  assert.ok(s.lines.every((l) => l.floorTagged && l.floor));
  const verify = s.lines.filter((l) => l.floor === "verify");
  assert.deepEqual([...new Set(verify.map((l) => l.deskRound))], [1, 2]);
  assert.ok(tl.rounds.some((r) => r.phase === "answer" && r.turns[0].reason === "referral"));
});

test("on a per-desk floor messages cross only as referrals, not as inferred crossings", () => {
  const tl = swarm();
  for (let n = 0; n <= tl.rounds.length; n++) assert.deepEqual(stateAt(tl, n).crossings, []);
});

test("referrals land on the step that was current when they streamed", () => {
  const tl = swarm();
  assert.ok(tl.referrals.length >= 3);
  assert.deepEqual(tl.referrals.map((r) => r.id), tl.referrals.map((_, i) => i));
  const first = tl.referrals[0];
  assert.equal(first.step, 1);
  assert.equal(first.accepted, true);
  const s1 = stateAt(tl, 1);
  assert.deepEqual(s1.fresh.map((r) => r.id), tl.referrals.filter((r) => r.step === 1).map((r) => r.id));
  assert.equal(stateAt(tl, 0).referrals.length, 0);
  const s2 = stateAt(tl, 2);
  assert.ok(s2.referrals.length > s2.fresh.length, "earlier referrals stay, only this step's are fresh");
  const refused = tl.referrals.filter((r) => !r.accepted);
  assert.ok(refused.some((r) => r.reason === "hop_limit_reached"));
  assert.ok(refused.some((r) => r.reason === "unlinked"));
});

test("transcript entries interleave turns, referrals and desk endings in stream order", () => {
  const tl = swarm();
  const s = stateAt(tl, tl.rounds.length);
  const kinds = s.entries.map((e) => e.kind);
  assert.equal(kinds.filter((k) => k === "turn").length, s.lines.length);
  assert.equal(kinds.filter((k) => k === "referral").length, tl.referrals.length);
  assert.equal(kinds.filter((k) => k === "desk_end").length, 5);
  const firstRef = kinds.indexOf("referral");
  assert.equal(s.entries[firstRef - 1].line.floor, "conjecture", "the report follows the desk that sent it");
});

test("desk endings show once streamed and the end names the answer desk", () => {
  const tl = swarm();
  const last = stateAt(tl, tl.rounds.length);
  assert.equal(last.deskEnds.size, 5);
  assert.equal(last.deskEnds.get("synthesis").ending, "converged");
  assert.equal(last.deskEnds.get("synthesis").correct, true);
  assert.equal(last.ending.desk, "synthesis");
  assert.equal(stateAt(tl, tl.rounds.length - 1).deskEnds.size, 0);
  const streaming = timeline(swarmFrames().filter((f) => f.type !== "end"));
  assert.equal(stateAt(streaming, streaming.rounds.length).deskEnds.size, 5, "a desk ending shows before the hive's end");
  assert.equal(stateAt(streaming, streaming.rounds.length).ending, null);
});

test("a live stream carries the model on start and usage before the end", () => {
  const live = swarmFrames({ live: true, model: "a/b:nitro" });
  assert.equal(live.at(-2).type, "usage");
  const tl = timeline(live);
  assert.equal(tl.start.live, true);
  assert.equal(tl.start.model, "a/b:nitro");
  assert.equal(stateAt(tl, 0).usage.calls, tl.end.turns);
  assert.equal(swarm().usage, null);
});

test("a per-desk turn saw only lines of its own floor", () => {
  const tl = swarm();
  const s = stateAt(tl, tl.rounds.length);
  const seat = s.lines.find((l) => l.floor === "verify" && l.deskRound === 2).seat;
  const view = sawBy(s, seat);
  for (const i of view.seen) assert.equal(s.lines[i].floor, "verify");
});

test("matches a referral to the declared link it travelled", () => {
  const st = structure("math-swarm");
  assert.deepEqual(edgeFor(st, "verify", "synthesis"), { from: "verify", to: "synthesis", kind: "referral", reversed: false });
  assert.deepEqual(edgeFor(st, "conjecture", "compute"), { from: "conjecture", to: "compute", kind: "topology", reversed: false });
  assert.deepEqual(edgeFor(st, "verify", "critique"), { from: "critique", to: "verify", kind: "topology", reversed: true });
  assert.equal(edgeFor(st, "synthesis", "conjecture"), null);
  assert.equal(edgeFor({}, "a", "b"), null);
});

test("words a referral, accepted or refused", () => {
  assert.equal(reasonText("hop_limit_reached"), "hop limit reached");
  assert.equal(reasonText("some_new_reason"), "some new reason");
  assert.equal(describeReferral({ kind: "report", from_desk: "a", to_desk: "b", target: "b-01", hops: 1, accepted: true }), "report a → b (b-01) · 1 hop");
  assert.equal(describeReferral({ kind: "forward", from_desk: "a", to_desk: "c", target: null, hops: 4, accepted: false, reason: "unlinked" }),
    "forward a → c refused: no link between these desks · 4 hops");
});

test("filters transcript entries to one desk", () => {
  const tl = swarm();
  const s = stateAt(tl, tl.rounds.length);
  const verify = s.entries.filter((e) => entryOnDesk(e, "verify"));
  assert.ok(verify.some((e) => e.kind === "turn") && verify.some((e) => e.kind === "referral") && verify.some((e) => e.kind === "desk_end"));
  assert.ok(verify.every((e) => e.kind !== "turn" || e.line.floor === "verify"));
  assert.ok(verify.filter((e) => e.kind === "referral").every((e) => e.ref.from_desk === "verify" || e.ref.to_desk === "verify"));
  assert.equal(s.entries.filter((e) => entryOnDesk(e, null)).length, s.entries.length);
  assert.equal(entryOnDesk({ kind: "mystery" }, "verify"), false);
  const tabs = deskTabs(structure("math-swarm"), s);
  assert.deepEqual(tabs, ["conjecture", "compute", "verify", "critique", "synthesis"]);
  assert.equal(deskCounts(s.entries, tabs).get("verify"), verify.length);
  assert.deepEqual(deskTabs({ desks: [{ id: "a" }] }, { entries: [{ kind: "turn", line: { floor: "b" } }] }), ["a", "b"]);
});

test("a single-desk stream with round.desk is unchanged apart from the floor", () => {
  const single = { desks: [{ id: "board" }] };
  const out = multiDeskFrames(frames("review-board.frames.ndjson"), single);
  assert.ok(!out.some((f) => f.type === "referral" || f.type === "desk_end"));
  assert.equal(out.at(-1).desk, undefined);
  const tl = timeline(out);
  assert.equal(stateAt(tl, tl.rounds.length).ending.topic, "ship");
});

test("a followed live stream stays on the newest round until the user steps back", () => {
  let last = null;
  const player = createPlayer((st) => { last = st; });
  player.reset(true, true);
  const all = swarmFrames({ live: true });
  for (const f of all.slice(0, 6)) player.push(f);
  assert.equal(last.following, true);
  assert.equal(last.playing, false);
  assert.equal(last.step, last.received);
  player.step(-1);
  assert.equal(last.following, false);
  const held = last.step;
  player.push(all[6]);
  assert.equal(player.state().step, held, "a scrubbed-back player is not dragged forward");
  player.latest();
  assert.equal(last.following, true);
  assert.equal(last.step, last.received);
  for (const f of all.slice(7)) player.push(f);
  assert.ok(player.state().ending, "following lands on the ending");
  player.finish();
  assert.equal(last.following, false);
  assert.equal(last.streaming, false);
  player.latest();
  assert.equal(last.following, false, "nothing to follow once the stream closed");
  player.pause();
});

test("standings use the current desk's own quorum, falling back to the package's", () => {
  const st = structure("math-swarm");
  assert.equal(deskPolicy(st, "verify").episode.quorum_threshold, st.episode.quorum_threshold, "no desks[].episode: the package policy");
  assert.equal(deskPolicy(st, "verify").name, "Verify");
  const routed = { ...st, desks: st.desks.map((d) => ({ ...d, episode: { turn_budget: 30, round_width: 10, revealed_width: 10, quorum_threshold: 6 } })) };
  assert.equal(deskPolicy(routed, "verify").episode.quorum_threshold, 6);
  assert.equal(deskPolicy(routed, "verify").episode.blind_round, st.episode.blind_round, "fields a desk leaves out come from the package");
  assert.deepEqual(deskPolicy(routed, null), { desk: null, name: null, episode: st.episode });
  assert.equal(deskPolicy(routed, "nowhere").desk, null);
});

test("pushMany folds a batch of frames and redraws once", () => {
  let draws = 0;
  let last = null;
  const player = createPlayer((st) => { draws++; last = st; });
  player.reset(true, true);
  const before = draws;
  player.pushMany([{ type: "start", package: "p" }, { type: "round", round: 1, turns: [] }, { type: "round", round: 2, turns: [] }]);
  assert.equal(draws - before, 1);
  assert.equal(last.received, 2);
  assert.equal(last.step, 2, "a followed stream lands on the newest round");
  player.pushMany([]);
  assert.equal(draws - before, 1, "an empty batch draws nothing");
});
