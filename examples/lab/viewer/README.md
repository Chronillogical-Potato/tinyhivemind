# Run viewer

Two tabs over one dev server. **Runs** is a Vite app for the telemetry a hive-lab run emits (`TraceEvent` in
`crates/tinyhivemind-core/src/telemetry/types.rs`, stamped with `run`, `seq`,
`at_ms`). The dev server reads traces straight from disk, so there is nothing
to upload: runs appear in a table, a click loads one, **Compare** loads every
arm of a task side by side, and a run that is still being written follows live.
**Hive** and **Compare** run the hive-language gallery packages offline through
the `hive_view` example and replay each episode round by round (see
[Hive tab](#hive-tab)).

```sh
cd examples/lab/viewer
npm install
HIVE_RUNS=/path/to/harbor/jobs npm run dev      # http://<host>:8099
```

`npm run dev` binds `0.0.0.0:8099` (`--strictPort`). `HIVE_RUNS` is a
colon-separated list of directories to scan for `*.jsonl` files (depth 6); the
default is `examples/lab/runs/` plus the bundled `public/fixtures/`. There is no
authentication, so anyone who can reach the port can read every trace under the
roots, and can start `hive_view` runs (see [Security](#security)).

| Path | Purpose |
| --- | --- |
| `index.html` | Page shell, styles, theme tokens |
| `src/main.js` | Parsing, analysis, SVG rendering (no chart libraries) |
| `src/panel.js` | The runs table: load, compare, live follow, URL state |
| `src/marks.js` | Which timeline lane a mark-like event goes to (`session_resumed` → session; `recalled`, `remembered`, failed-call marks → memory; other marks) and its tooltip title |
| `src/marks.test.js` | `npm test`: lanes, titles and memory-error classification |
| `server/scan.js` | Finds and describes traces; resolves ids safely |
| `server/plugin.js` | Vite plugin serving `/api/runs`, `/api/runs/<id>`, `/api/events` |
| `server/scan.test.js` | `npm test`: scanner, arm and path-escape tests |
| `server/hives.js` | Hive packages: listing, name and knob validation, argv, NDJSON splitting, the `hive_view` runner |
| `server/hive-api.js` | The `/api/hives` and `/api/compare` routes |
| `server/hives.test.js` | `npm test`: listing, path escape, knob bounds, argv, line splitting, streaming and kill |
| `src/hive/` | The Hive and Compare tabs; see [`src/hive/README.md`](src/hive/README.md) |
| `tools/probe.mjs`, `tools/live-check.mjs` | Headless-Chromium checks over the DevTools protocol |
| `tools/hive-check.mjs` | Headless check of the Hive and Compare tabs; saves screenshots |
| `tools/fake-hive-view.mjs` | Replays the hive fixtures with `hive_view`'s argv, for work without cargo |
| `public/fixtures/sample.jsonl` | Two demo runs: `hive-4seat` and `baseline-serial` |
| `public/fixtures/hives/` | Hand-written `hive_view` output: structures, frame streams and a compare stream (`.ndjson`, so the runs scanner skips them) |

## How runs are found

A Harbor trial (`<job>/<task>__<hash>/agent/trace.jsonl`) is described from its
`agent/result.json` (mode, tokens, wall time, turns) and the trial's
`result.json` (verifier reward). Jobs named `<tag>-<mode>` (for example
`r1-single` and `r1-hive`) share the tag `r1`, so one task's arms land on one
row. The arm is everything after the tag, so `harbor/arm.sh` jobs such as
`r2-hive-briefing` and `r2-hive-session-mem` get their own columns. Any other `.jsonl` is listed under its folder, by file name.

## API

- `GET /api/runs` returns `{roots, runs: [{id, path, group, tag, task, mode, arm,
  reward, tokens, wall_ms, turns, size, mtime}]}`, newest first.
- `GET /api/runs/<id>` returns the raw trace. Ids are `<root index>:<relative
  path>`; anything that resolves outside a root, or is not `.jsonl`, is a 404.
- `GET /api/events` is a server-sent-event stream; a `runs` event fires when a
  trace appears or changes size (polled every second). The page re-reads the
  list and re-fetches any loaded trace that grew.

## URL state

`?run=<id>&run=<id>` reopens the same selection. With none, the page opens the
newest task that has two arms in Compare. `?file=<url>` still fetches any
static file. Drag and drop of local files still works, and is all a static
`npm run build` supports (there is no `/api` there, so the panel hides).

## Views

1. **Waterfall**: a lane per seat. A `turn_started` is matched to its
   `turn_finished` by the `turn` id (seat order when the id is absent). Overlapping turns of one seat
   stack. A finish with no start is reconstructed from `latency_ms`; a start
   with no finish runs to the end of the run (dashed). Tool calls sit in their
   turn's row (by `turn`), spanning `[at_ms - latency_ms, at_ms]`; refused calls
   are red with a cross and the `reason` in the tooltip.
2. **Timeline**: rounds (phase and visibility; dashed = blind), one row per
   conductor kind, outcome markers (C converged, D deadlocked, E exhausted,
   I idle), then three mark lanes: `session` (green circle per
   `session_resumed`), `memory` (purple diamond per `recalled` or
   `remembered`; red for a `memory` mark reporting a failed or timed-out
   call) and every other mark (triangle) or checkpoint (square). Traces from
   before the typed events used `session` / `memory` marks; they land in the
   same lanes.
3. **Tokens**: cumulative input/output/total over time, plus per-seat share.
4. **Summary**: turns, tokens in/out, wall ms, p50/p95 turn latency (from
   `latency_ms`, nearest rank), max concurrent turns, tool calls, refusals,
   rounds, conductor events, outcome. With two or more runs, pick A and B for a
   delta column (B minus A).

Hover or focus any mark for its raw event; Escape dismisses. Each run also has
an event table. Unreadable lines are counted and skipped.

## Timing

`at_ms` is the start for `turn_started` and the end for `turn_finished` and
`tool_call`, as documented in core. Time zero is the earliest event of a run.

## Hive tab

`?view=hive` opens a gallery with one card per package under
`crates/tinyhivemind-lang/examples/hives/` (or `HIVE_PACKAGES`). **Run** opens
the player and streams one episode from
`hive_view <package> --run --frames`:

- **Controls**: package, seed, round width, turn budget (blank means the
  package's own policy) and, for math packages, the problem. **Play/Pause**,
  **Step** (or the arrow keys), speed 0.5× to 8× and the scrubber replay the
  buffered rounds, so you can rewind while the stream is still arriving.
- **Room**: desks are boxes and seats are nodes in their first desk. Topology
  links are solid arrows, referral links dashed. The current round's seats
  glow, with a dashed ring in a blind round and a solid one in a full round,
  and an edge the conversation crossed between two rounds animates. Speech
  bubbles show what each speaker said. Above 16 seats the nodes go compact
  (numbered, unlabelled) and only a few speakers get bubbles; hover or select
  a seat for its own. In a math package a seat turns green or red once its
  latest answer is right or wrong.
- **Quorum standings**: support and object per topic, a dashed line at the
  quorum threshold, and "committed" once the room records it.
- **Transcript**: click a seat (or a name in the transcript) to highlight its
  latest turn and the lines it saw: the `saw` most recent of the `of` lines in
  front of it, counting only earlier rounds for a blind turn.
- An ending banner reports converged, deadlocked, exhausted or idle, the topic,
  and for a math package whether it was correct.

`?hive=<name>&seed=N` (plus `round_width`, `turn_budget`, `problem`) reopens
and reruns a player.

## Compare tab

`?compare=math-panel,math-swarm&seeds=N` runs
`hive_view <first package> --compare --seeds N --packages a,b` and streams one
result per seed. The single-seat baseline is always included. The page draws
accuracy, convergence and mean rounds per package, and a per-seed strip
(green correct, red wrong, dark no decision). Clicking a cell opens that seed
and problem in the player.

## Hive API

- `GET /api/hives` returns `{root, packages: [{name, id, title, seats, desks,
  summary, kind}]}`. Structures come from `hive_view <dir> --json`, cached by
  the package's newest file mtime.
- `GET /api/hives/<name>` returns that structure.
- `GET /api/hives/<name>/run?seed=&round_width=&turn_budget=&problem=` is a
  server-sent-event stream: `status`, then one `frame` per NDJSON line, then
  `done`, or `error` with `{message, stderr}` (the stderr tail) on a non-zero
  exit.
- `GET /api/compare?packages=a,b&seeds=N` streams `seed` events and a final
  `summary`, then `done` or `error`.

The first request builds the binary once with
`cargo build -p tinyhivemind-lang --example hive_view` (concurrent requests
share one build) and then runs `target/debug/examples/hive_view`.
`HIVE_PACKAGES` replaces the package root. `HIVE_VIEW_BIN` replaces the binary
and skips the build. A `.mjs` runs under node, so
`HIVE_VIEW_BIN=tools/fake-hive-view.mjs` replays the fixtures.

## Security

There is no authentication. Anyone who can reach the port can read every
trace under the roots and make the server run `hive_view`, which costs CPU.
Hive runs are fenced in: every process is spawned from an argv array with no
shell, `<name>` must be a package from the listing (anything else is a 404,
`..` included), and every knob is a plain integer inside its bounds (seed
0..2^32, round width 1..64, turn budget 1..2000, seeds 1..200) or the request
is a 400. A problem id must be short and alphanumeric. The child is killed
when the client disconnects. Bind to localhost, or keep the port off untrusted
networks.

## Checks

```sh
npm test                    # node --test: server and pure page modules
npm run build               # static page (Runs tab drag and drop only)
chrome --headless=new --remote-debugging-port=9333 --user-data-dir=/somewhere &
node tools/hive-check.mjs http://localhost:8099/ shots   # SCHEME=light for light mode
```

`hive-check.mjs` opens the gallery, `?hive=review-board`, `?hive=math-swarm`
and `?compare=math-panel,math-swarm&seeds=10`, waits for each ending, replays
a seed from the compare strip, and writes PNGs to `shots/` (git-ignored).
