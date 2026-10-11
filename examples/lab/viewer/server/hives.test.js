import assert from "node:assert/strict";
import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import { fileURLToPath } from "node:url";
import { test } from "node:test";
import { hiveApi } from "./hive-api.js";
import {
  LIVE_MODEL_DEFAULT, card, compareArgs, findPackage, hiveRunner, lineSplitter, listPackageDirs, liveAvailable, liveModelDefault,
  packagesRoot, parseKnobs, parseLive, repoRoot, runArgs, sse, validModel,
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
  assert.ok(lines.length < 9, `got ${lines.length} of the stream's frames`);
});

// A stand-in for the key: tests never see or need a real one.
const KEYED = { OPENROUTER_API_KEY: "test-placeholder" };

test("live is available only when the server's env carries a non-empty key", () => {
  assert.equal(liveAvailable({}), false);
  assert.equal(liveAvailable({ OPENROUTER_API_KEY: "" }), false);
  assert.equal(liveAvailable({ OPENROUTER_API_KEY: "   " }), false);
  assert.equal(liveAvailable(KEYED), true);
  assert.equal(liveModelDefault({}), LIVE_MODEL_DEFAULT);
  assert.equal(liveModelDefault({ OPENROUTER_MODEL: "anthropic/claude-x" }), "anthropic/claude-x");
  assert.equal(liveModelDefault({ OPENROUTER_MODEL: "x/y:online" }), LIVE_MODEL_DEFAULT, "a web-search default is not offered");
});

test("accepts plain model ids and refuses web-search variants and flag-like ids", () => {
  for (const ok of ["openai/gpt-oss-120b:nitro", "anthropic/claude-3.5-sonnet", "meta-llama/llama-3.1-8b-instruct:free", "x"]) {
    assert.equal(validModel(ok), true, ok);
  }
  for (const bad of ["openai/gpt-4o:online", "openai/gpt-4o:ONLINE", "perplexity/sonar-online", "x/y:web", "--help", "-m", "a b", "a;rm", "a:b:c", "", null, "a".repeat(101), "a/b:" + "c".repeat(31)]) {
    assert.equal(validModel(bad), false, String(bad));
  }
});

test("parses the live knobs, bounding max_calls and refusing live without a key", () => {
  const q = (s) => new URLSearchParams(s);
  assert.deepEqual(parseLive(q(""), KEYED), { ok: true, value: null });
  assert.deepEqual(parseLive(q("live=0&model=x:online"), {}), { ok: true, value: null }, "offline ignores the model");
  assert.equal(parseLive(q("live=1"), {}).ok, false);
  assert.match(parseLive(q("live=1"), {}).error, /unavailable/);
  assert.deepEqual(parseLive(q("live=1"), KEYED).value, { model: LIVE_MODEL_DEFAULT, max_calls: 300 });
  assert.deepEqual(parseLive(q("live=1&model=a/b&max_calls=1"), KEYED).value, { model: "a/b", max_calls: 1 });
  assert.equal(parseLive(q("live=1&max_calls=300"), KEYED).value.max_calls, 300);
  for (const bad of ["live=1&max_calls=0", "live=1&max_calls=301", "live=1&max_calls=2000", "live=1&max_calls=-1", "live=1&max_calls=1.5",
    "live=1&model=openai/gpt-4o:online", "live=1&model=--max-calls", "live=yes"]) {
    assert.equal(parseLive(q(bad), KEYED).ok, false, bad);
  }
});

test("puts live knobs in separate argv elements and never the key", () => {
  const args = runArgs("/p/ms", { seed: 2 }, null, { model: "a/b:nitro", max_calls: 50 });
  assert.deepEqual(args, ["/p/ms", "--run", "--frames", "--seed", "2", "--live", "--model", "a/b:nitro", "--max-calls", "50"]);
  assert.ok(!args.some((a) => a.includes("test-placeholder")));
});

test("a multi-desk run streams desk-tagged rounds, referrals and desk endings", async () => {
  const root = gallery("math-swarm");
  const runner = hiveRunner({ repo: here, env: { HIVE_VIEW_BIN: FAKE } });
  const frames = [];
  const { code } = await runner.stream(runArgs(path.join(root, "math-swarm")), (l) => frames.push(JSON.parse(l))).done;
  assert.equal(code, 0);
  assert.ok(frames.filter((f) => f.type === "round").every((f) => f.desk && f.desk_round >= 1));
  const refs = frames.filter((f) => f.type === "referral");
  assert.ok(refs.some((r) => r.accepted) && refs.some((r) => !r.accepted && r.reason));
  assert.equal(frames.filter((f) => f.type === "desk_end").length, 5);
  assert.equal(frames.at(-1).desk, "synthesis");
  assert.ok(!frames.some((f) => f.type === "usage"), "usage is live only");
});

test("a live run gets the key through env only and ends with a usage frame", async () => {
  const root = gallery("review-board");
  const dir = path.join(root, "review-board");
  const live = parseLive(new URLSearchParams("live=1&max_calls=20"), KEYED).value;
  const keyed = hiveRunner({ repo: here, env: { HIVE_VIEW_BIN: FAKE, ...KEYED } });
  const frames = [];
  const { code } = await keyed.stream(runArgs(dir, {}, null, live), (l) => frames.push(JSON.parse(l))).done;
  assert.equal(code, 0);
  assert.equal(frames[0].live, true);
  assert.equal(frames[0].model, LIVE_MODEL_DEFAULT);
  assert.equal(frames.at(-2).type, "usage");
  const bare = hiveRunner({ repo: here, env: { HIVE_VIEW_BIN: FAKE } });
  const refused = await bare.stream(runArgs(dir, {}, null, live), () => {}).done;
  assert.notEqual(refused.code, 0);
  assert.match(refused.stderr, /OPENROUTER_API_KEY/);
});

test("a stream past its time cap is killed and reported as timed out", async () => {
  const root = gallery("math-swarm");
  const runner = hiveRunner({ repo: here, env: { HIVE_VIEW_BIN: FAKE } });
  const { code, timedOut } = await runner.stream(runArgs(path.join(root, "math-swarm")), () => {}, { maxMs: 30 }).done;
  assert.notEqual(code, 0);
  assert.equal(timedOut, true);
});

/** A just-enough ServerResponse: records status, headers and body. */
function fakeRes() {
  const res = { statusCode: 200, headers: {}, body: "", ended: null, handlers: {} };
  res.setHeader = (k, v) => { res.headers[k] = v; };
  res.writeHead = (code, h) => { res.statusCode = code; Object.assign(res.headers, h); };
  res.write = (chunk) => { res.body += chunk; };
  res.on = (ev, fn) => { res.handlers[ev] = fn; };
  res.ended = new Promise((resolve) => { res.end = (chunk = "") => { res.body += chunk; resolve(res); }; });
  return res;
}

test("the listing reports live availability and the default model, never the key", async () => {
  const root = gallery("review-board");
  for (const [env, available] of [[{}, false], [KEYED, true]]) {
    const api = hiveApi({ repo: here, env: { HIVE_VIEW_BIN: FAKE, HIVE_PACKAGES: root, OPENROUTER_MODEL: "a/b:nitro", ...env } });
    const res = fakeRes();
    await api.handle("/hives", new URLSearchParams(), {}, res);
    const body = JSON.parse((await res.ended).body);
    assert.equal(body.live_available, available);
    assert.equal(body.live_model_default, "a/b:nitro");
    assert.ok(!JSON.stringify(body).includes("test-placeholder"));
  }
});

test("the run route refuses live without a key and bad live knobs with a 400", async () => {
  const root = gallery("review-board");
  const run = async (env, qs) => {
    const api = hiveApi({ repo: here, env: { HIVE_VIEW_BIN: FAKE, HIVE_PACKAGES: root, ...env } });
    const res = fakeRes();
    await api.handle("/hives/review-board/run", new URLSearchParams(qs), {}, res);
    return res.ended;
  };
  assert.equal((await run({}, "live=1")).statusCode, 400);
  assert.equal((await run(KEYED, "live=1&model=openai/gpt-4o:online")).statusCode, 400);
  assert.equal((await run(KEYED, "live=1&max_calls=301")).statusCode, 400);
  const ok = await run(KEYED, "live=1&max_calls=5");
  assert.equal(ok.statusCode, 200);
  assert.match(ok.body, /"type":"usage"/);
  assert.match(ok.body, /event: done/);
});
