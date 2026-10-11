// The `/api/hives` and `/api/compare` routes. Every name is checked against
// the listing and every knob against its bounds before anything is spawned;
// anything else is a 400 or 404. Streams are server-sent events and the child
// is killed when the client goes away. Every line is written the moment the
// child prints it, and a comment line every KEEPALIVE_MS keeps a quiet live
// run (minutes between rounds) from looking dead to a proxy or the browser.
import {
  LIVE_TIMEOUT_MS, card, compareArgs, findPackage, hiveRunner, listPackageDirs, liveAvailable, liveModelDefault, packagesRoot,
  parseKnobs, parseLive, runArgs, sse, validProblem,
} from "./hives.js";

const KEEPALIVE_MS = 15000;

function send(res, status, body) {
  res.statusCode = status;
  res.setHeader("Content-Type", typeof body === "string" ? "text/plain; charset=utf-8" : "application/json");
  res.end(typeof body === "string" ? body : JSON.stringify(body));
}

/** `{ root, handle(pathname, query, req, res) -> boolean }` for the hive routes. */
export function hiveApi({ repo, env = process.env, log = () => {} }) {
  const root = packagesRoot(env, repo);
  const runner = hiveRunner({ repo, env, log });

  async function listing() {
    const dirs = listPackageDirs(root);
    const cards = [];
    for (const d of dirs) {
      try {
        cards.push(card(d.name, await runner.structure(d.dir)));
      } catch (err) {
        cards.push({ ...card(d.name, {}), error: err.message });
      }
    }
    return cards;
  }

  // Forward each stdout line of `args` as `event`, then `done`, or `error` with the stderr tail.
  function streamTo(res, args, eventFor, { maxMs = 0 } = {}) {
    res.writeHead(200, { "Content-Type": "text/event-stream", "Cache-Control": "no-store", Connection: "keep-alive", "X-Accel-Buffering": "no" });
    res.flushHeaders?.();
    res.write(sse("status", { state: "starting" }));
    let closed = false;
    const keepalive = setInterval(() => { if (!closed) res.write(": keepalive\n\n"); }, KEEPALIVE_MS);
    const run = runner.stream(args, (line) => {
      let parsed;
      try {
        parsed = JSON.parse(line);
      } catch {
        return; // a stray human-readable line; frames are one JSON object per line
      }
      if (!closed) res.write(sse(eventFor(parsed), line));
    }, { maxMs });
    // `res` closes when the client goes away (or after we end it); either way
    // the child has nothing left to say to anyone.
    res.on("close", () => {
      closed = true;
      clearInterval(keepalive);
      run.kill();
    });
    run.done
      .then(({ code, stderr, timedOut }) => {
        clearInterval(keepalive);
        if (closed) return;
        if (code === 0) res.write(sse("done", {}));
        else if (timedOut) res.write(sse("error", { message: `hive_view ran past ${Math.round(maxMs / 60000)} minutes and was stopped`, stderr }));
        else res.write(sse("error", { message: `hive_view exited ${code}`, stderr }));
        res.end();
      })
      .catch((err) => {
        clearInterval(keepalive);
        if (closed) return;
        res.write(sse("error", { message: err.message, stderr: "" }));
        res.end();
      });
  }

  async function handle(pathname, query, req, res) {
    if (pathname === "/hives") {
      send(res, 200, { root, packages: await listing(), live_available: liveAvailable(env), live_model_default: liveModelDefault(env) });
      return true;
    }
    const m = /^\/hives\/([^/]+)(\/run)?$/.exec(pathname);
    if (m) {
      const pkg = findPackage(listPackageDirs(root), decodeURIComponent(m[1]));
      if (!pkg) return send(res, 404, "no such package"), true;
      if (!m[2]) {
        try {
          send(res, 200, await runner.structure(pkg.dir));
        } catch (err) {
          send(res, 500, err.message);
        }
        return true;
      }
      const knobs = parseKnobs(query, ["seed", "round_width", "turn_budget"]);
      if (!knobs.ok) return send(res, 400, knobs.error), true;
      const problem = query.get("problem") || null;
      if (problem && !validProblem(problem)) return send(res, 400, "bad problem id"), true;
      const live = parseLive(query, env);
      if (!live.ok) return send(res, 400, live.error), true;
      streamTo(res, runArgs(pkg.dir, knobs.value, problem, live.value), () => "frame", { maxMs: live.value ? LIVE_TIMEOUT_MS : 0 });
      return true;
    }
    if (pathname === "/compare") {
      const dirs = listPackageDirs(root);
      const names = (query.get("packages") || "math-panel,math-swarm").split(",").filter(Boolean);
      const pkgs = names.map((n) => findPackage(dirs, n));
      if (!names.length || names.length > 8 || pkgs.some((p) => !p)) return send(res, 400, "unknown package"), true;
      const knobs = parseKnobs(query, ["seeds"]);
      if (!knobs.ok) return send(res, 400, knobs.error), true;
      const args = compareArgs(pkgs.map((p) => p.dir), names, knobs.value.seeds ?? 20);
      streamTo(res, args, (o) => (o.type === "summary" ? "summary" : "seed"));
      return true;
    }
    return false;
  }

  return { root, handle };
}
