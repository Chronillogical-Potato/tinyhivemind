// Hive packages for the Hive tab: finds the gallery on disk, runs the
// `hive_view` example against one package, and streams what it prints. The
// pure parts (validation, knob bounds, argv, line splitting) are exported on
// their own so they are testable without a binary.
import { spawn } from "node:child_process";
import fs from "node:fs";
import path from "node:path";

const NAME = /^[A-Za-z0-9][A-Za-z0-9._-]{0,63}$/;
const STDERR_TAIL = 4000;

/** Inclusive bounds for each numeric knob, keyed by query parameter. */
export const KNOBS = {
  seed: { flag: "--seed", min: 0, max: 2 ** 32 },
  round_width: { flag: "--round-width", min: 1, max: 64 },
  turn_budget: { flag: "--turn-budget", min: 1, max: 2000 },
  seeds: { flag: "--seeds", min: 1, max: 200 },
};

/** The repository root, four levels above `examples/lab/viewer/server`. */
export function repoRoot(serverDir) {
  return path.resolve(serverDir, "../../../..");
}

/** Where the gallery lives: HIVE_PACKAGES, or the lang crate's examples/hives. */
export function packagesRoot(env, repo) {
  return path.resolve(env.HIVE_PACKAGES || path.join(repo, "crates/tinyhivemind-lang/examples/hives"));
}

/** Package directories under `root` that hold a `hive.json`, sorted by name. */
export function listPackageDirs(root) {
  let entries;
  try {
    entries = fs.readdirSync(root, { withFileTypes: true });
  } catch {
    return [];
  }
  return entries
    .filter((e) => e.isDirectory() && NAME.test(e.name) && fs.existsSync(path.join(root, e.name, "hive.json")))
    .map((e) => ({ name: e.name, dir: path.join(root, e.name) }))
    .sort((a, b) => a.name.localeCompare(b.name));
}

/** The listed package called `name`, or null for anything else (including `..`). */
export function findPackage(dirs, name) {
  if (typeof name !== "string" || !NAME.test(name)) return null;
  return dirs.find((d) => d.name === name) ?? null;
}

/**
 * Parse the knobs in `keys` from a URLSearchParams. Absent knobs are left out;
 * a present one must be a plain integer inside its bounds.
 * Returns `{ ok: true, value }` or `{ ok: false, error }`.
 */
export function parseKnobs(query, keys) {
  const value = {};
  for (const key of keys) {
    const raw = query.get(key);
    if (raw == null || raw === "") continue;
    const { min, max } = KNOBS[key];
    if (!/^\d{1,10}$/.test(raw)) return { ok: false, error: `${key} must be an integer` };
    const n = Number(raw);
    if (n < min || n > max) return { ok: false, error: `${key} must be within ${min}..${max}` };
    value[key] = n;
  }
  return { ok: true, value };
}

/** A math problem id: short, plain, never a flag. */
export function validProblem(id) {
  return typeof id === "string" && /^[A-Za-z0-9][A-Za-z0-9._-]{0,63}$/.test(id);
}

const knobArgs = (knobs) => Object.entries(knobs).flatMap(([k, v]) => [KNOBS[k].flag, String(v)]);

/** argv for the structure of one package. */
export function jsonArgs(dir) {
  return [dir, "--json"];
}

/** argv for one framed episode run. */
export function runArgs(dir, knobs = {}, problem = null) {
  return [dir, "--run", "--frames", ...knobArgs(knobs), ...(problem ? ["--problem", problem] : [])];
}

/** argv for a compare stream over `packages` (directories), first one positional. */
export function compareArgs(dirs, names, seeds) {
  return [dirs[0], "--compare", "--seeds", String(seeds), "--packages", names.join(",")];
}

/** Splits a byte stream into complete lines; `push` returns the lines a chunk finished. */
export function lineSplitter() {
  let rest = "";
  return {
    push(chunk) {
      rest += chunk;
      const lines = rest.split(/\r?\n/);
      rest = lines.pop();
      return lines.filter((l) => l.trim());
    },
    flush() {
      const last = rest.trim();
      rest = "";
      return last ? [last] : [];
    },
  };
}

/** One server-sent event. */
export function sse(event, data) {
  const body = typeof data === "string" ? data : JSON.stringify(data);
  return `event: ${event}\n${body.split("\n").map((l) => `data: ${l}`).join("\n")}\n\n`;
}

/** Newest modification time of the files in a package (two levels deep). */
export function packageMtime(dir, depth = 0) {
  let newest = 0;
  for (const e of fs.readdirSync(dir, { withFileTypes: true })) {
    const full = path.join(dir, e.name);
    if (e.isFile()) newest = Math.max(newest, fs.statSync(full).mtimeMs);
    else if (e.isDirectory() && depth < 2) newest = Math.max(newest, packageMtime(full, depth + 1));
  }
  return newest;
}

/** The gallery card for one structure. */
export function card(name, structure) {
  return {
    name,
    id: structure.id ?? name,
    title: structure.name ?? name,
    seats: structure.seats?.length ?? 0,
    desks: structure.desks?.length ?? 0,
    summary: structure.summary ?? "",
    kind: structure.sim?.kind ?? "script",
  };
}

/**
 * Runs `hive_view`: builds it once (one shared promise, so concurrent first
 * requests wait on one cargo), caches structures by package mtime, and spawns
 * runs with an argv array and no shell. HIVE_VIEW_BIN swaps the binary (a
 * `.mjs` runs under node), which skips the build; tests and fixtures use it.
 */
export function hiveRunner({ repo, env = process.env, log = () => {} }) {
  const override = env.HIVE_VIEW_BIN;
  const bin = override ? path.resolve(override) : path.join(repo, "target/debug/examples/hive_view");
  const cache = new Map(); // dir -> {mtime, structure}
  let built = null;

  function command(args) {
    return /\.m?js$/.test(bin) ? [process.execPath, [bin, ...args]] : [bin, args];
  }

  function ensureBuilt() {
    if (override) return Promise.resolve();
    built ??= new Promise((resolve, reject) => {
      log("building hive_view (cargo build -p tinyhivemind-lang --example hive_view)");
      const child = spawn("cargo", ["build", "-p", "tinyhivemind-lang", "--example", "hive_view"], { cwd: repo, stdio: ["ignore", "ignore", "pipe"] });
      let err = "";
      child.stderr.on("data", (d) => { err = (err + d).slice(-STDERR_TAIL); });
      child.on("error", (e) => reject(new Error(`cargo: ${e.message}`)));
      child.on("close", (code) => (code === 0 ? resolve() : reject(new Error(`cargo build failed (${code}):\n${err}`))));
    }).catch((e) => {
      built = null; // let the next request retry
      throw e;
    });
    return built;
  }

  function capture(args) {
    return new Promise((resolve, reject) => {
      const [cmd, argv] = command(args);
      const child = spawn(cmd, argv, { cwd: repo, stdio: ["ignore", "pipe", "pipe"] });
      let out = "", err = "";
      child.stdout.on("data", (d) => { out += d; });
      child.stderr.on("data", (d) => { err = (err + d).slice(-STDERR_TAIL); });
      child.on("error", reject);
      child.on("close", (code) => (code === 0 ? resolve(out) : reject(new Error(err.trim() || `hive_view exited ${code}`))));
    });
  }

  async function structure(dir) {
    const mtime = packageMtime(dir);
    const hit = cache.get(dir);
    if (hit && hit.mtime === mtime) return hit.structure;
    await ensureBuilt();
    const parsed = JSON.parse(await capture(jsonArgs(dir)));
    cache.set(dir, { mtime, structure: parsed });
    return parsed;
  }

  /** Spawn `args` and call `onLine` per stdout line; resolves to {code, stderr}. Returns {done, kill}. */
  function stream(args, onLine) {
    let child = null;
    let killed = false;
    const done = ensureBuilt().then(
      () =>
        new Promise((resolve) => {
          if (killed) return resolve({ code: null, stderr: "" });
          const [cmd, argv] = command(args);
          child = spawn(cmd, argv, { cwd: repo, stdio: ["ignore", "pipe", "pipe"] });
          const lines = lineSplitter();
          let err = "";
          child.stdout.setEncoding("utf8");
          child.stdout.on("data", (d) => lines.push(d).forEach(onLine));
          child.stderr.on("data", (d) => { err = (err + d).slice(-STDERR_TAIL); });
          child.on("error", (e) => resolve({ code: -1, stderr: e.message }));
          child.on("close", (code) => {
            lines.flush().forEach(onLine);
            resolve({ code, stderr: err.trim() });
          });
        }),
    );
    return {
      done,
      kill() {
        killed = true;
        child?.kill("SIGTERM");
      },
    };
  }

  return { bin, ensureBuilt, structure, stream };
}
