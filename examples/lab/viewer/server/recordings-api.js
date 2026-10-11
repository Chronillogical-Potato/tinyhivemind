// The `/api/recordings` routes: the listing, one recording's structure, and
// its frames as server-sent events that keep tailing the file while the run
// is still being written. Ids go through `resolveRecording`, so nothing
// outside a root is ever read.
import fs from "node:fs";
import { sse } from "./hives.js";
import { describeRecording, resolveRecording, scanRecordings, tailFrames } from "./recordings.js";

const KEEPALIVE_MS = 15000;

function send(res, status, body) {
  res.statusCode = status;
  res.setHeader("Content-Type", typeof body === "string" ? "text/plain; charset=utf-8" : "application/json");
  res.end(typeof body === "string" ? body : JSON.stringify(body));
}

/**
 * `{ handle(pathname, req, res) -> boolean }`. `packageStructure(name)`
 * resolves to the `--json` structure of a gallery package, or null; it stands
 * in for a recording that carries no `structure.json` (a `hive_view --live`
 * run). A recording with neither is left out of the listing.
 */
export function recordingsApi({ roots, packageStructure = async () => null, intervalMs = 500 }) {
  async function hasStructure(rec) {
    if (rec.structure) return true;
    try {
      return !!(await packageStructure(rec.package));
    } catch {
      return false;
    }
  }

  async function listing() {
    const all = scanRecordings(roots);
    const keep = await Promise.all(all.map(hasStructure));
    return all.filter((_, i) => keep[i]);
  }

  function frames(rec, req, res) {
    res.writeHead(200, { "Content-Type": "text/event-stream", "Cache-Control": "no-store", Connection: "keep-alive", "X-Accel-Buffering": "no" });
    res.flushHeaders?.();
    const info = describeRecording(Number(rec.id.split(":")[0]), roots[Number(rec.id.split(":")[0])], rec.frames);
    res.write(sse("status", { live: info.live, ended: info.ended }));
    let closed = false;
    const keepalive = setInterval(() => { if (!closed) res.write(": keepalive\n\n"); }, KEEPALIVE_MS);
    // A run that is neither ended nor live crashed or was killed: send what
    // it wrote and stop, rather than tail a file that will never grow.
    const tail = tailFrames(rec.frames, {
      intervalMs,
      follow: info.live,
      onLine: (line) => { if (!closed) res.write(sse("frame", line)); },
      onEnd: ({ ended }) => {
        clearInterval(keepalive);
        if (closed) return;
        res.write(sse("done", { ended }));
        res.end();
      },
    });
    res.on("close", () => {
      closed = true;
      clearInterval(keepalive);
      tail.stop();
    });
  }

  async function handle(pathname, req, res) {
    if (pathname === "/recordings") {
      send(res, 200, { roots, recordings: await listing() });
      return true;
    }
    const m = /^\/recordings\/(.+)\/(structure|frames)$/.exec(pathname);
    if (!m) return false;
    let id;
    try {
      id = decodeURIComponent(m[1]);
    } catch {
      return send(res, 400, "bad recording id"), true;
    }
    const found = resolveRecording(roots, id);
    if (!found) return send(res, 404, "no such recording"), true;
    if (m[2] === "frames") {
      frames({ id, frames: found.frames }, req, res);
      return true;
    }
    if (found.structure) {
      try {
        send(res, 200, JSON.parse(fs.readFileSync(found.structure, "utf8")));
      } catch (err) {
        send(res, 500, `bad structure.json: ${err.message}`);
      }
      return true;
    }
    const rec = describeRecording(Number(id.split(":")[0]), roots[Number(id.split(":")[0])], found.frames);
    const st = await packageStructure(rec.package).catch(() => null);
    if (!st) return send(res, 404, `no structure.json and no package named ${rec.package}`), true;
    send(res, 200, st);
    return true;
  }

  return { handle };
}
