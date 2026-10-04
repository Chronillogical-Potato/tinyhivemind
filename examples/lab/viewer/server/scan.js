// Finds run traces on disk and describes them. Plain functions over the
// filesystem, so the plugin stays thin and the logic is testable.
import fs from "node:fs";
import path from "node:path";

const MAX_DEPTH = 6;
const SKIP = new Set(["node_modules", ".git", "target", "dist"]);

/** Directories to scan: HIVE_RUNS (path-delimited) or the fallback list. */
export function rootsFromEnv(env, fallback) {
  const raw = env.HIVE_RUNS;
  const list = raw ? raw.split(path.delimiter).filter(Boolean) : fallback;
  return list.map((r) => path.resolve(r)).filter((r) => fs.existsSync(r));
}

function readJson(file) {
  try {
    return JSON.parse(fs.readFileSync(file, "utf8"));
  } catch {
    return null;
  }
}

function* walk(dir, depth = 0) {
  if (depth > MAX_DEPTH) return;
  let entries;
  try {
    entries = fs.readdirSync(dir, { withFileTypes: true });
  } catch {
    return;
  }
  for (const e of entries) {
    const full = path.join(dir, e.name);
    if (e.isDirectory() && !SKIP.has(e.name)) yield* walk(full, depth + 1);
    else if (e.isFile() && e.name.endsWith(".jsonl")) yield full;
  }
}

/**
 * Describe one trace. A Harbor trial lays out `<job>/<task>__<hash>/agent/trace.jsonl`
 * with `agent/result.json` (our summary) and `../result.json` (Harbor's, with the
 * verifier reward); anything else is a plain file named by its path.
 */
export function describe(rootIndex, root, file) {
  const rel = path.relative(root, file).split(path.sep).join("/");
  const st = fs.statSync(file);
  const dir = path.dirname(file);
  const own = readJson(path.join(dir, "result.json"));
  const trial = path.basename(dir) === "agent" ? readJson(path.join(dir, "..", "result.json")) : null;
  const parts = rel.split("/");
  const harbor = trial && parts.length >= 4;
  const mode = own && typeof own.mode === "string" ? own.mode : null;
  const reward = trial?.verifier_result?.rewards?.reward ?? null;
  const group = harbor ? parts[0] : parts.length > 1 ? parts.slice(0, -1).join("/") : path.basename(root);
  const task = harbor ? parts[1].split("__")[0] : path.basename(file, ".jsonl");
  const tag = mode && group.endsWith(`-${mode}`) ? group.slice(0, -mode.length - 1) : group;
  return {
    id: `${rootIndex}:${rel}`,
    path: rel,
    group,
    tag,
    task,
    mode,
    reward: typeof reward === "number" ? reward : null,
    tokens: own ? (own.tokens_in ?? 0) + (own.tokens_out ?? 0) : null,
    wall_ms: own?.wall_ms ?? null,
    turns: own?.turns ?? null,
    size: st.size,
    mtime: Math.round(st.mtimeMs),
  };
}

/** Every trace under the roots, newest first. */
export function scan(roots) {
  const out = [];
  roots.forEach((root, i) => {
    for (const file of walk(root)) out.push(describe(i, root, file));
  });
  return out.sort((a, b) => b.mtime - a.mtime);
}

/** Resolve a run id to a file inside a root, or null (also for path escapes). */
export function resolveId(roots, id) {
  const m = /^(\d+):(.+)$/.exec(id ?? "");
  const root = m && roots[Number(m[1])];
  if (!root) return null;
  const file = path.resolve(root, m[2]);
  const inside = file.startsWith(root + path.sep);
  return inside && file.endsWith(".jsonl") && fs.existsSync(file) ? file : null;
}
