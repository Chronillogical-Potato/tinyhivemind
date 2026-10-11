// The `/api/recordings` routes: the listing, one recording's structure, and
// its frames as server-sent events that keep tailing the file while the run
// is still being written. Ids go through `resolveRecording`, so nothing
// outside a root is ever read.
import fs from "node:fs";
import { sse } from "./hives.js";
import { describeRecording, resolveRecording, scanRecordings, tailFrames } from "./recordings.js";

const KEEPALIVE_MS = 15000;
/** How often `/api/recordings/events` rescans while a page listens. */
export const WATCH_MS = 1000;

/** What changes when a recording appears, grows, goes quiet or ends. */
export const listingSignature = (list) => list.map((r) => `${r.id}:${r.size}:${r.mtime}:${r.live}:${r.ended}`).join("|");

function send(res, status, body) {
  res.statusCode = status;
  res.setHeader("Content-Type", typeof body === "string" ? "text/plain; charset=utf-8" : "application/json");
  res.end(typeof body === "string" ? body : JSON.stringify(body));
}

/**
 * `{ handle(pathname, req, res) -> boolean }`. `packageStructure(name)`
 * resolves to the `--json` structure of a gallery package, or null; it stands
 * in for a recording that carries no `structure.json` (a `hive_view --live`
 * run); `hasPackage(name)` says cheaply whether one exists. A recording with
 * neither is left out of the listing.
 */
export function recordingsApi({ roots, packageStructure = async () => null, hasPackage = () => false, intervalMs = 500, watchMs = WATCH_MS }) {
  async function listing() {
    return scanRecordings(roots).filter((rec) => rec.structure || hasPackage(rec.package));
  }

  // `/api/recordings/events`: one `recordings` event carrying the whole
  // listing now, and again whenever it changes. One timer serves every page
  // and runs only while one listens.
  const watchers = new Set();
  let timer = null;
  let last = "";
  async function poll() {
    const list = await listing();
    const sig = listingSignature(list);
    if (sig !== last) {
      last = sig;
      const event = sse("recordings", { recordings: list });
      for (const res of watchers) res.write(event);
    }
  }
  async function watch(req, res) {
    res.writeHead(200, { "Content-Type": "text/event-stream", "Cache-Control": "no-store", Connection: "keep-alive", "X-Accel-Buffering": "no" });
    res.flushHeaders?.();
    res.write(sse("recordings", { recordings: await listing() }));
    watchers.add(res);
    const keepalive = setInterval(() => res.write(": keepalive\n\n"), KEEPALIVE_MS);
    timer ??= setInterval(() => { poll().catch(() => {}); }, watchMs);
    res.on("close", () => {
      watchers.delete(res);
      clearInterval(keepalive);
      if (!watchers.size) {
        clearInterval(timer);
        timer = null;
        last = "";
      }
    });
  }

  function frames(found, res) {
    res.writeHead(200, { "Content-Type": "text/event-stream", "Cache-Control": "no-store", Connection: "keep-alive", "X-Accel-Buffering": "no" });
    res.flushHeaders?.();
    const info = describeRecording(found.rootIndex, found.root, found.frames);
    res.write(sse("status", { live: info.live, active: info.active, ended: info.ended }));
    let closed = false;
    const keepalive = setInterval(() => { if (!closed) res.write(": keepalive\n\n"); }, KEEPALIVE_MS);
    // A run with no end frame that has not grown for STALE_MS crashed or was
    // killed: send what it wrote and stop, rather than tail it forever.
    const tail = tailFrames(found.frames, {
      intervalMs,
      follow: info.active,
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
    if (pathname === "/recordings/events") {
      await watch(req, res);
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
      frames(found, res);
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
    const rec = describeRecording(found.rootIndex, found.root, found.frames);
    const st = await packageStructure(rec.package).catch(() => null);
    if (!st) return send(res, 404, `no structure.json and no package named ${rec.package}`), true;
    send(res, 200, st);
    return true;
  }

  /** Stop the watch timer (the dev server is closing). */
  const close = () => { clearInterval(timer); timer = null; };

  return { handle, close };
}
