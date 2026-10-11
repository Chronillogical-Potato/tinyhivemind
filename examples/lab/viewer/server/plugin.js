// Vite plugin: `/api/runs` lists traces, `/api/runs/<id>` returns one as text,
// and `/api/events` is a server-sent-event stream that fires when the set of
// traces (or any trace's size) changes, so a run in progress shows up live.
// `/api/hives` and `/api/compare` run hive packages; see hive-api.js.
// `/api/recordings` lists and replays recorded hive runs; see recordings-api.js.
import fs from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";
import { hiveApi } from "./hive-api.js";
import { repoRoot } from "./hives.js";
import { recordingsApi } from "./recordings-api.js";
import { mainCheckout, recordingRoots } from "./recordings.js";
import { resolveId, rootsFromEnv, scan } from "./scan.js";

const here = path.dirname(fileURLToPath(import.meta.url));
const POLL_MS = 1000;

export function runsPlugin() {
  const roots = rootsFromEnv(process.env, [path.resolve(here, "../../runs"), path.resolve(here, "../public/fixtures")]);
  const signature = (runs) => runs.map((r) => `${r.id}:${r.size}:${r.mtime}`).join("|");
  return {
    name: "hive-lab-runs",
    configureServer(server) {
      const clients = new Set();
      let last = signature(scan(roots));
      const timer = setInterval(() => {
        if (!clients.size) return;
        const now = signature(scan(roots));
        if (now === last) return;
        last = now;
        for (const res of clients) res.write("event: runs\ndata: changed\n\n");
      }, POLL_MS);
      server.httpServer?.on("close", () => clearInterval(timer));
      server.config.logger.info(`  runs: ${roots.length ? roots.join(", ") : "(none; set HIVE_RUNS)"}`);
      const hives = hiveApi({ repo: repoRoot(here), log: (m) => server.config.logger.info(`  hives: ${m}`) });
      server.config.logger.info(`  hives: ${hives.root}`);
      const recRoots = recordingRoots(process.env, [path.join(mainCheckout(repoRoot(here)), "hive-runs"), path.resolve(here, "../public/fixtures/recordings")]);
      const recordings = recordingsApi({ roots: recRoots, packageStructure: hives.structureOf, hasPackage: hives.hasPackage });
      server.config.logger.info(`  recordings: ${recRoots.length ? recRoots.join(", ") : "(none; set HIVE_RECORDINGS)"}`);

      server.middlewares.use("/api", (req, res, next) => {
        const url = new URL(req.url, "http://x");
        res.setHeader("Cache-Control", "no-store");
        if (url.pathname === "/hives" || url.pathname.startsWith("/hives/") || url.pathname === "/compare") {
          hives.handle(url.pathname, url.searchParams, req, res).catch((err) => {
            if (!res.headersSent) res.statusCode = 500;
            res.end(String(err.message));
          });
        } else if (url.pathname === "/recordings" || url.pathname.startsWith("/recordings/")) {
          recordings.handle(url.pathname, req, res).then((handled) => handled || next()).catch((err) => {
            if (!res.headersSent) res.statusCode = 500;
            res.end(String(err.message));
          });
        } else if (url.pathname === "/runs") {
          res.setHeader("Content-Type", "application/json");
          res.end(JSON.stringify({ roots, runs: scan(roots) }));
        } else if (url.pathname.startsWith("/runs/")) {
          const file = resolveId(roots, decodeURIComponent(url.pathname.slice(6)));
          if (!file) {
            res.statusCode = 404;
            res.end("no such run");
            return;
          }
          res.setHeader("Content-Type", "text/plain; charset=utf-8");
          fs.createReadStream(file).pipe(res);
        } else if (url.pathname === "/events") {
          res.writeHead(200, { "Content-Type": "text/event-stream", Connection: "keep-alive" });
          res.write(": connected\n\n");
          clients.add(res);
          req.on("close", () => clients.delete(res));
        } else {
          next();
        }
      });
    },
  };
}
