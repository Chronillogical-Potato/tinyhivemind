// A stand-in for the `hive_view` example that replays the bundled fixtures in
// public/fixtures/hives/, so the Hive tab can be worked on without cargo.
// Point the dev server at it with HIVE_VIEW_BIN=tools/fake-hive-view.mjs and
// HIVE_PACKAGES at a folder of `<name>/hive.json` directories named after the
// fixtures (review-board, math-swarm). It speaks the same argv as hive_view.
import fs from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

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
  for (const line of read(`${name}.frames.ndjson`).split("\n").filter(Boolean)) {
    const frame = JSON.parse(line);
    if (frame.type === "start") frame.seed = seed;
    process.stdout.write(JSON.stringify(frame) + "\n");
    await sleep(15);
  }
} else {
  fail("usage: fake-hive-view <dir> --json | --run --frames | --compare");
}
