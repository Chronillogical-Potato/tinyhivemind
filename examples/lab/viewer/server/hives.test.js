import assert from "node:assert/strict";
import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import { fileURLToPath } from "node:url";
import { test } from "node:test";
import {
  card, compareArgs, findPackage, hiveRunner, lineSplitter, listPackageDirs, packagesRoot, parseKnobs, repoRoot, runArgs, sse,
} from "./hives.js";

const here = path.dirname(fileURLToPath(import.meta.url));
const FAKE = path.resolve(here, "../tools/fake-hive-view.mjs");

function gallery(...names) {
  const root = fs.mkdtempSync(path.join(os.tmpdir(), "hive-pkgs-"));
  for (const n of names) {
    fs.mkdirSync(path.join(root, n));
    fs.writeFileSync(path.join(root, n, "hive.json"), "{}");
  }
  return root;
}

test("finds the repository root above the viewer", () => {
  const repo = repoRoot(here);
  assert.ok(fs.existsSync(path.join(repo, "Cargo.toml")));
  assert.equal(packagesRoot({}, repo), path.join(repo, "crates/tinyhivemind-lang/examples/hives"));
  assert.equal(packagesRoot({ HIVE_PACKAGES: "/x/y" }, repo), "/x/y");
});

test("lists only directories that hold a hive.json, sorted", () => {
  const root = gallery("zeta", "alpha");
  fs.mkdirSync(path.join(root, "empty"));
  fs.writeFileSync(path.join(root, "loose.json"), "{}");
  assert.deepEqual(listPackageDirs(root).map((d) => d.name), ["alpha", "zeta"]);
  assert.deepEqual(listPackageDirs(path.join(root, "missing")), []);
});

test("accepts only listed names and refuses path escapes", () => {
  const dirs = listPackageDirs(gallery("review-board"));
  assert.equal(findPackage(dirs, "review-board").name, "review-board");
  for (const bad of ["..", "../review-board", "review-board/..", "/etc", "", "a b", "unlisted", null]) {
    assert.equal(findPackage(dirs, bad), null, String(bad));
  }
});

test("bounds every numeric knob and rejects anything that is not an integer", () => {
  const q = (s) => new URLSearchParams(s);
  assert.deepEqual(parseKnobs(q("seed=0&round_width=64&turn_budget=2000"), ["seed", "round_width", "turn_budget"]), {
    ok: true,
    value: { seed: 0, round_width: 64, turn_budget: 2000 },
  });
  assert.deepEqual(parseKnobs(q("seed=4294967296"), ["seed"]).value, { seed: 2 ** 32 });
  assert.deepEqual(parseKnobs(q(""), ["seed"]).value, {});
  for (const bad of ["seed=4294967297", "seed=-1", "seed=1.5", "seed=1e3", "seed=--help", "round_width=0", "round_width=65", "turn_budget=2001"]) {
    const key = bad.split("=")[0];
    assert.equal(parseKnobs(q(bad), [key]).ok, false, bad);
  }
  assert.equal(parseKnobs(q("seeds=201"), ["seeds"]).ok, false);
  assert.equal(parseKnobs(q("seeds=200"), ["seeds"]).ok, true);
});

test("builds argv arrays with flags in the contract's spelling", () => {
  assert.deepEqual(runArgs("/p/rb", { seed: 3, round_width: 2, turn_budget: 9 }, "lattice-6x6"), [
    "/p/rb", "--run", "--frames", "--seed", "3", "--round-width", "2", "--turn-budget", "9", "--problem", "lattice-6x6",
  ]);
  assert.deepEqual(runArgs("/p/rb"), ["/p/rb", "--run", "--frames"]);
  assert.deepEqual(compareArgs(["/p/a", "/p/b"], ["a", "b"], 10), ["/p/a", "--compare", "--seeds", "10", "--packages", "a,b"]);
});

test("splits NDJSON lines across chunk boundaries", () => {
  const s = lineSplitter();
  assert.deepEqual(s.push('{"a":'), []);
  assert.deepEqual(s.push('1}\n{"b"'), ['{"a":1}']);
  assert.deepEqual(s.push(":2}\r\n\n{\"c\":3}\n"), ['{"b":2}', '{"c":3}']);
  assert.deepEqual(s.push('{"d":4}'), []);
  assert.deepEqual(s.flush(), ['{"d":4}']);
  assert.deepEqual(s.flush(), []);
});

test("frames a server-sent event per data line", () => {
  assert.equal(sse("frame", '{"a":1}'), 'event: frame\ndata: {"a":1}\n\n');
  assert.equal(sse("error", { message: "x" }), 'event: error\ndata: {"message":"x"}\n\n');
  assert.equal(sse("e", "a\nb"), "event: e\ndata: a\ndata: b\n\n");
});

test("describes a package card from its structure", () => {
  const s = JSON.parse(fs.readFileSync(path.resolve(here, "../public/fixtures/hives/math-swarm.json"), "utf8"));
  assert.deepEqual(card("math-swarm", s), {
    name: "math-swarm", id: "math-swarm", title: "Math swarm", seats: 50, desks: 5, summary: s.summary, kind: "math",
  });
  assert.equal(card("x", {}).kind, "script");
});

test("streams a run through the binary and caches structures by mtime", async () => {
  const root = gallery("review-board");
  const runner = hiveRunner({ repo: here, env: { HIVE_VIEW_BIN: FAKE } });
  const dir = path.join(root, "review-board");
  const a = await runner.structure(dir);
  assert.equal(a.id, "review-board");
  assert.equal(await runner.structure(dir), a, "second read is the cached object");
  const lines = [];
  const { code } = await runner.stream(runArgs(dir, { seed: 9 }), (l) => lines.push(JSON.parse(l))).done;
  assert.equal(code, 0);
  assert.equal(lines[0].type, "start");
  assert.equal(lines[0].seed, 9);
  assert.equal(lines.at(-1).type, "end");
});

test("reports a failing run with its stderr", async () => {
  const root = gallery("unknown-pkg");
  const runner = hiveRunner({ repo: here, env: { HIVE_VIEW_BIN: FAKE } });
  const { code, stderr } = await runner.stream(runArgs(path.join(root, "unknown-pkg")), () => {}).done;
  assert.notEqual(code, 0);
  assert.match(stderr, /no fixture/);
  await assert.rejects(runner.structure(path.join(root, "unknown-pkg")), /no fixture/);
});

test("killing a stream stops the child before it finishes", async () => {
  const root = gallery("math-swarm");
  const runner = hiveRunner({ repo: here, env: { HIVE_VIEW_BIN: FAKE } });
  const lines = [];
  const run = runner.stream(runArgs(path.join(root, "math-swarm")), (l) => {
    lines.push(l);
    run.kill();
  });
  const { code } = await run.done;
  assert.notEqual(code, 0);
  assert.ok(lines.length < 9, `got ${lines.length} of 9 frames`);
});
