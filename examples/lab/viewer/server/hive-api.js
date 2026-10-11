// The `/api/hives` and `/api/compare` routes. Every name is checked against
// the listing and every knob against its bounds before anything is spawned;
// anything else is a 400 or 404. Streams are server-sent events and the child
// is killed when the client goes away.
import {
  card, compareArgs, findPackage, hiveRunner, listPackageDirs, packagesRoot, parseKnobs, runArgs, sse, validProblem,
} from "./hives.js";

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
  function streamTo(req, res, args, eventFor) {
    res.writeHead(200, { "Content-Type": "text/event-stream", "Cache-Control": "no-store", Connection: "keep-alive" });
    res.write(sse("status", { state: "starting" }));
    let closed = false;
    const run = runner.stream(args, (line) => {
      let parsed;
      try {
        parsed = JSON.parse(line);
      } catch {
        return; // a stray human-readable line; frames are one JSON object per line
      }
      if (!closed) res.write(sse(eventFor(parsed), line));
    });
    req.on("close", () => {
      closed = true;
      run.kill();
    });
    run.done
      .then(({ code, stderr }) => {
        if (closed) return;
        if (code === 0) res.write(sse("done", {}));
        else res.write(sse("error", { message: `hive_view exited ${code}`, stderr }));
        res.end();
      })
      .catch((err) => {
        if (closed) return;
        res.write(sse("error", { message: err.message, stderr: "" }));
        res.end();
      });
  }

  async function handle(pathname, query, req, res) {
    if (pathname === "/hives") {
      send(res, 200, { root, packages: await listing() });
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
      streamTo(req, res, runArgs(pkg.dir, knobs.value, problem), () => "frame");
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
      streamTo(req, res, args, (o) => (o.type === "summary" ? "summary" : "seed"));
      return true;
    }
    return false;
  }

  return { root, handle };
}
