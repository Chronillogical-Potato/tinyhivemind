# Run viewer

A Vite app for the telemetry a hive-lab run emits (`TraceEvent` in
`crates/tinyhivemind-core/src/telemetry/types.rs`, stamped with `run`, `seq`,
`at_ms`). The dev server reads traces straight from disk, so there is nothing
to upload: runs appear in a table, a click loads one, **Compare** loads every
arm of a task side by side, and a run that is still being written follows live.

```sh
cd examples/lab/viewer
npm install
HIVE_RUNS=/path/to/harbor/jobs npm run dev      # http://<host>:8099
```

`npm run dev` binds `0.0.0.0:8099` (`--strictPort`). `HIVE_RUNS` is a
colon-separated list of directories to scan for `*.jsonl` files (depth 6); the
default is `examples/lab/runs/` plus the bundled `public/fixtures/`. There is no
authentication, so anyone who can reach the port can read every trace under the
roots.

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
| `tools/probe.mjs`, `tools/live-check.mjs` | Headless-Chromium checks over the DevTools protocol |
| `public/fixtures/sample.jsonl` | Two demo runs: `hive-4seat` and `baseline-serial` |

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
