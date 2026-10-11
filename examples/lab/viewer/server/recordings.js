// Recorded hive runs on disk: a run directory holds `frames.ndjson` (the
// `hive_view --run --frames` wire contract, one frame per line) and, when the
// host wrote one, `structure.json` (the `--json` structure). Hosts such as the
// OpenHuman hives write both; `hive_view --live` writes frames only, so its
// structure is reconstructed from the package its `start` frame names (see
// recordings-api.js). Plain functions over the filesystem, so the scan, the id
// check and the tail are testable without a server.
import fs from "node:fs";
import path from "node:path";
import { StringDecoder } from "node:string_decoder";

export const FRAMES = "frames.ndjson";
export const STRUCTURE = "structure.json";
/** A recording whose file changed this recently, with no `end` frame yet, is live. */
export const LIVE_MS = 30_000;
/**
 * A recording with no `end` frame is tailed until it has not grown for this
 * long: a live model round can take minutes, so `live` (30 s) is too short a
 * leash for the tail, while a run that crashed should not be followed forever.
 */
export const STALE_MS = 10 * 60_000;
const MAX_DEPTH = 5;
const SKIP = new Set(["node_modules", ".git", "target", "dist"]);

/**
 * The main checkout a repository belongs to: `repo` itself, unless its `.git`
 * is a file pointing at a linked worktree's git dir carrying a `commondir`, in
 * which case the parent of the shared `.git` dir. Mirrors hive_view's
 * `main_checkout`, so the viewer reads the `hive-runs/` hive_view writes.
 */
export function mainCheckout(repo) {
  try {
    const pointer = fs.readFileSync(path.join(repo, ".git"), "utf8");
    const m = /^gitdir:\s*(.+)$/m.exec(pointer);
    if (!m) return repo;
    const gitdir = path.resolve(repo, m[1].trim());
    const common = path.resolve(gitdir, fs.readFileSync(path.join(gitdir, "commondir"), "utf8").trim());
    return path.dirname(fs.realpathSync(common));
  } catch {
    return repo;
  }
}

/** Roots to scan: HIVE_RECORDINGS (colon-separated) or `fallback`; only directories that exist. */
export function recordingRoots(env, fallback) {
  const raw = env.HIVE_RECORDINGS;
  const list = raw ? raw.split(":").filter(Boolean) : fallback;
  return [...new Set(list.map((r) => path.resolve(r)))].filter((r) => {
    try {
      return fs.statSync(r).isDirectory();
    } catch {
      return false;
    }
  });
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
    if (e.isFile() && e.name === FRAMES) yield path.join(dir, e.name);
    else if (e.isDirectory() && !SKIP.has(e.name)) yield* walk(path.join(dir, e.name), depth + 1);
  }
}

const summaries = new Map(); // file -> { key, value }

/**
 * What the frames say: `{ frames, start, end, round, desk, turns, tools,
 * calls }`, counting complete JSON lines: the newest round and its desk, the
 * turns and tool calls so far, and the model calls (`usage.calls` once it
 * has arrived, else one per turn). Cached by size and mtime, so a listing
 * rereads only files that grew.
 */
export function summarizeFrames(file, st = fs.statSync(file)) {
  const key = `${st.size}:${st.mtimeMs}`;
  const hit = summaries.get(file);
  if (hit && hit.key === key) return hit.value;
  let frames = 0, start = null, end = null, round = null, desk = null, turns = 0, tools = 0, calls = null;
  for (const line of fs.readFileSync(file, "utf8").split("\n")) {
    if (!line.trim()) continue;
    let f;
    try {
      f = JSON.parse(line);
    } catch {
      continue; // a partial last line of a run still being written
    }
    frames++;
    if (f?.type === "start" && !start) start = f;
    else if (f?.type === "end") end = f;
    else if (f?.type === "usage") calls = f.calls ?? null;
    else if (f?.type === "round") {
      round = f.round ?? round;
      desk = f.desk ?? desk;
      for (const t of f.turns ?? []) {
        turns++;
        if (Array.isArray(t.tool_calls)) tools += t.tool_calls.length;
      }
    }
  }
  const value = { frames, start, end, round, desk, turns, tools, calls: calls ?? turns };
  summaries.set(file, { key, value });
  return value;
}

/** Milliseconds a run directory's name ends with (`<package>-seed<N>-<ms>`), or null. */
function stampFromName(dir) {
  const m = /-(\d{12,14})$/.exec(path.basename(dir));
  return m ? Number(m[1]) : null;
}

/** Describe the recording whose frames are `file`, under root number `rootIndex`. */
export function describeRecording(rootIndex, root, file, now = Date.now()) {
  const st = fs.statSync(file);
  const dir = path.dirname(file);
  const rel = path.relative(root, dir).split(path.sep).join("/") || ".";
  const { frames, start, end, round, desk, turns, tools, calls } = summarizeFrames(file, st);
  const mtime = Math.round(st.mtimeMs);
  const problem = start?.problem ? { id: start.problem.id ?? null, statement: start.problem.statement ?? null } : null;
  return {
    id: `${rootIndex}:${rel}`,
    path: rel,
    package: typeof start?.package === "string" ? start.package : path.basename(dir),
    problem,
    model: start?.model ?? null,
    seed: start?.seed ?? null,
    round,
    desk,
    turns,
    tool_calls: tools,
    calls,
    started: stampFromName(dir) ?? Math.round(st.birthtimeMs || st.ctimeMs),
    frames,
    size: st.size,
    mtime,
    live: !end && now - mtime < LIVE_MS,
    active: !end && now - mtime < STALE_MS,
    ended: !!end,
    ending: end?.ending ?? null,
    correct: end?.correct ?? null,
    structure: fs.existsSync(path.join(dir, STRUCTURE)),
  };
}

/** Every recording under `roots`, newest change first. */
export function scanRecordings(roots, now = Date.now()) {
  const out = [];
  roots.forEach((root, i) => {
    for (const file of walk(root)) {
      try {
        out.push(describeRecording(i, root, file, now));
      } catch {
        // vanished between readdir and stat
      }
    }
  });
  return out.sort((a, b) => b.mtime - a.mtime);
}

/**
 * Resolve a recording id to `{ rootIndex, root, dir, frames, structure }`
 * (`structure` null when absent) inside its root, or
 * null: for a malformed id, an unknown root, a path that escapes the root
 * (also through a symlink), or a directory without `frames.ndjson`.
 */
export function resolveRecording(roots, id) {
  const m = /^(\d+):(.+)$/.exec(typeof id === "string" ? id : "");
  const root = m && roots[Number(m[1])];
  if (!root || m[2].includes("\0")) return null;
  const dir = path.resolve(root, m[2]);
  const frames = path.join(dir, FRAMES);
  try {
    const realRoot = fs.realpathSync(root);
    const real = fs.realpathSync(frames);
    if (!real.startsWith(realRoot + path.sep) || path.basename(real) !== FRAMES || !fs.statSync(real).isFile()) return null;
  } catch {
    return null;
  }
  const structure = path.join(dir, STRUCTURE);
  return { rootIndex: Number(m[1]), root, dir, frames, structure: fs.existsSync(structure) ? structure : null };
}

/**
 * Follow `file` from byte 0: `onLine(text)` for every complete line already
 * there and for every line appended later, polled every `intervalMs`. A
 * partial last line waits until its newline arrives (or, once the run is
 * over, is flushed as is). Stops after a line whose frame is `end`, calling
 * `onEnd({ ended: true })`; with `follow: false`, or once the file has not
 * grown for `staleMs`, it stops after the content there with `onEnd({ ended })`.
 * Returns `{ stop() }`.
 */
export function tailFrames(file, { onLine, onEnd = () => {}, intervalMs = 500, follow = true, staleMs = STALE_MS, now = Date.now }) {
  const decoder = new StringDecoder("utf8");
  let offset = 0;
  let rest = "";
  let stopped = false;
  let timer = null;
  let grew = now();

  const finish = (ended) => {
    if (stopped) return;
    stopped = true;
    clearTimeout(timer);
    onEnd({ ended });
  };

  // Emit one line; true when it was the end frame.
  const emit = (line) => {
    if (!line.trim()) return false;
    onLine(line);
    try {
      return JSON.parse(line)?.type === "end";
    } catch {
      return false;
    }
  };

  function read() {
    let fd;
    try {
      fd = fs.openSync(file, "r");
      const size = fs.fstatSync(fd).size;
      if (size < offset) { offset = 0; rest = ""; } // truncated and rewritten
      const buf = Buffer.alloc(64 * 1024);
      while (offset < size && !stopped) {
        const n = fs.readSync(fd, buf, 0, Math.min(buf.length, size - offset), offset);
        if (n <= 0) break;
        offset += n;
        grew = now();
        const lines = (rest + decoder.write(buf.subarray(0, n))).split("\n");
        rest = lines.pop();
        for (const l of lines) if (emit(l.replace(/\r$/, ""))) return finish(true);
      }
    } catch {
      // the file can vanish or be replaced mid-run; try again next poll
    } finally {
      if (fd != null) fs.closeSync(fd);
    }
    if (stopped) return;
    if (!follow || now() - grew >= staleMs) {
      const last = rest + decoder.end();
      rest = "";
      return finish(emit(last));
    }
    timer = setTimeout(read, intervalMs);
  }

  read();
  return { stop: () => { stopped = true; clearTimeout(timer); } };
}
