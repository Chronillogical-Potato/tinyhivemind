// A stand-in for the `hive_view` example that replays the bundled fixtures in
// public/fixtures/hives/, so the Hive tab can be worked on without cargo.
// Point the dev server at it with HIVE_VIEW_BIN=tools/fake-hive-view.mjs and
// HIVE_PACKAGES at a folder of `<name>/hive.json` directories named after the
// fixtures (review-board, math-swarm). It speaks the same argv as hive_view:
// a multi-desk package streams one floor per desk with referral and desk_end
// frames (see fake-frames.mjs), and `--live` (which, like the binary, needs a
// non-empty OPENROUTER_API_KEY in its env and refuses `:online` models) adds
// `start.live`/`model` and a `usage` frame. It never calls a model.
import fs from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";
import { multiDeskFrames } from "./fake-frames.mjs";

const fixtures = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "../public/fixtures/hives");
const [dir, ...args] = process.argv.slice(2);
const name = path.basename(dir ?? "");
const flag = (f) => args.indexOf(f) >= 0;
const value = (f) => (flag(f) ? args[args.indexOf(f) + 1] : null);
const fail = (msg) => {
  process.stderr.write(`${msg}\n`);
  process.exit(2);
};
const read = (file) => {
  const full = path.join(fixtures, file);
  if (!fs.existsSync(full)) fail(`no fixture for ${name}: ${file}`);
  return fs.readFileSync(full, "utf8");
};
const sleep = (ms) => new Promise((r) => setTimeout(r, ms));

if (flag("--json")) {
  process.stdout.write(JSON.stringify(JSON.parse(read(`${name}.json`))) + "\n");
} else if (flag("--compare")) {
  const seeds = Number(value("--seeds") ?? 20);
  for (const line of read("compare.ndjson").split("\n").filter(Boolean).filter((l) => JSON.parse(l).seed == null || JSON.parse(l).seed <= seeds)) {
    process.stdout.write(line + "\n");
    await sleep(15);
  }
} else if (flag("--run") && flag("--frames")) {
  const seed = Number(value("--seed") ?? 1);
  const live = flag("--live");
  const model = value("--model") ?? "openai/gpt-oss-120b:nitro";
  if (live && !process.env.OPENROUTER_API_KEY) fail("--live needs OPENROUTER_API_KEY in the environment");
  if (live && /online|:web/i.test(model)) fail(`model ${model} has web search; refusing`);
  const maxCalls = Number(value("--max-calls") ?? 300);
  if (live && !(maxCalls >= 1 && maxCalls <= 2000)) fail("--max-calls must be within 1..=2000");
  const structure = JSON.parse(read(`${name}.json`));
  const fixture = read(`${name}.frames.ndjson`).split("\n").filter(Boolean).map((l) => JSON.parse(l));
  for (const frame of multiDeskFrames(fixture, structure, { live, model })) {
    if (frame.type === "start") frame.seed = seed;
    process.stdout.write(JSON.stringify(frame) + "\n");
    await sleep(15);
  }
} else {
  fail("usage: fake-hive-view <dir> --json | --run --frames [--live --model <id> --max-calls N] | --compare");
}
