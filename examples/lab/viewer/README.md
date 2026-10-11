# Run viewer

Two tabs over one dev server. **Runs** is a Vite app for the telemetry a hive-lab run emits (`TraceEvent` in
`crates/tinyhivemind-core/src/telemetry/types.rs`, stamped with `run`, `seq`,
`at_ms`). The dev server reads traces straight from disk, so there is nothing
to upload: runs appear in a table, a click loads one, **Compare** loads every
arm of a task side by side, and a run that is still being written follows live.
**Hive** and **Compare** run the hive-language gallery packages offline through
the `hive_view` example and replay each episode round by round (see
[Hive tab](#hive-tab)). The Hive tab also lists every **recorded run** on this
machine and follows one that is still going (see [Recordings](#recordings)).

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
| `server/hives.js` | Hive packages: listing, name, knob and live-knob validation, argv, NDJSON splitting, the `hive_view` runner |
| `server/hive-api.js` | The `/api/hives` and `/api/compare` routes |
| `server/hives.test.js` | `npm test`: listing, path escape, knob bounds, live availability, model and call-cap validation, argv, line splitting, streaming, kill and timeout |
| `server/recordings.js` | Recordings: the main-checkout resolution, scanning `hive-runs/` for `frames.ndjson`, live detection, safe ids, and tailing a file still being written |
| `server/recordings-api.js` | The `/api/recordings` routes, including the pushed `events` stream |
| `server/recordings.test.js` | `npm test`: scan depth, live and stale, partial lines, id safety, symlink escape, commondir, tailing (split lines, split UTF-8, stale stop), the routes and the events stream |
| `src/hive/` | The Hive and Compare tabs; see [`src/hive/README.md`](src/hive/README.md) |
| `tools/probe.mjs`, `tools/live-check.mjs` | Headless-Chromium checks over the DevTools protocol |
| `tools/hive-check.mjs` | Headless check of the Hive and Compare tabs; saves screenshots |
| `tools/fake-hive-view.mjs` | Replays the hive fixtures with `hive_view`'s argv (including `--live`, which needs a key in its env but calls no model), for work without cargo |
| `tools/fake-frames.mjs` | Pure: rewrites a hive-floor fixture into the multi-desk stream (per-desk rounds, referrals, desk endings, live usage); shared by the fake and the page tests |
| `public/fixtures/sample.jsonl` | Two demo runs: `hive-4seat` and `baseline-serial` |
| `public/fixtures/hives/` | Hand-written `hive_view` output: structures, frame streams and a compare stream (`.ndjson`, so the runs scanner skips them) |
| `public/fixtures/recordings/` | One hand-made recording (`coin-panel`, two desks) whose turns carry `tool_calls` and `memory`, one of them long enough to fold |

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
- **Live (OpenRouter)**: tick it to run seats as real model calls instead of
  the offline simulation, with a model id (default: what the server reports,
  `OPENROUTER_MODEL` or `openai/gpt-oss-120b:nitro`) and a call cap (default
  and maximum 300). It is disabled, with a note, when the dev server has no
  `OPENROUTER_API_KEY`. A live run takes minutes; the player follows the
  stream, jumping to each round as it arrives, until you pause, step or
  scrub back (**Latest ⇥** resumes following). When the run ends it replays
  like any other, and the model usage (calls, tokens, cost, wall time) shows
  above the room. Live knobs are never written to the URL, so a reload cannot
  start a billed run.
- **Room**: desks are boxes and seats are nodes in their first desk. Topology
  links are solid arrows, referral links dashed. The current round's seats
  glow, with a dashed ring in a blind round and a solid one in a full round.
  A multi-desk package runs one floor per desk, so messages cross desks only
  as `referral` frames: each is a packet travelling the declared link it
  used. An accepted one reaches the target desk and lights the link; a
  refused one is red, stops short and bounces back, labelled with the reason
  (hover for the full referral), on a temporary dashed red edge when the
  package declares no link between the two desks. A one-floor stream without
  referrals still animates an edge the conversation crossed between rounds.
  Each desk that ended carries a badge (its ending and topic); the desk whose
  ending is the hive's answer is outlined green. Speech
  bubbles show what each speaker said. Above 16 seats the nodes go compact
  (numbered, unlabelled) and only a few speakers get bubbles; hover or select
  a seat for its own. In a math package a seat turns green or red once its
  latest answer is right or wrong.
- **Quorum standings**: support and object per topic, a dashed line at the
  quorum threshold, and "committed" once the room records it. In a multi-desk
  run the panel follows the current round's desk and uses that desk's own
  quorum (`desks[].episode`, else the package's).
- **Transcript**: click a seat (or a name in the transcript) to highlight its
  latest turn and the lines it saw: the `saw` most recent of the `of` lines in
  front of it, counting only earlier rounds for a blind turn and only its own
  desk's floor in a multi-desk run. A multi-desk package gets tabs (All, then
  one per desk); referral rows (incoming, outgoing, or blocked with the
  reason) and desk endings sit between the turns in stream order.
- An ending banner reports converged, deadlocked, exhausted or idle, the topic,
  for a math package whether it was correct, and for a multi-desk package
  which desk's ending is the answer, followed by a chip per desk ending.

`?hive=<name>&seed=N` (plus `round_width`, `turn_budget`, `problem`) reopens
and reruns a player.

## Recordings

A recording is a run directory holding `frames.ndjson` (the `hive_view --run
--frames` wire contract) and usually `structure.json` (the `--json`
structure). `hive_view --live` and `hive_view --run --record` write both into
`hive-runs/hive-view/<package>-seed<N>-<ms>/`, every run started from this
page is recorded the same way, and other hosts (such as the OpenHuman hives)
write their own folders under `hive-runs/`. Turns may carry extra fields the
page shows: `tool_calls: [{tool, summary, ok}]` and `memory: {recalled,
remembered}` (counts or lists).

The server scans `HIVE_RECORDINGS` (colon-separated) or, by default, the
main checkout's `hive-runs/` (a linked worktree's `.git` file is followed
through `commondir`, as `hive_view` does) plus `public/fixtures/recordings/`,
to depth 5. A folder with no `structure.json` is listed only when its `start`
frame names a gallery package, whose structure stands in.

- **Live now**: a strip at the top of the Hive tab shows every run still
  being written: package, problem or seed, newest round and desk, calls so
  far and elapsed time, with a pulsing **LIVE** badge (changed in the last
  30 s) or **QUIET** (no end frame, frames written, quiet for under 10
  minutes; a live round can take that long). Click one to follow it at its
  newest round, or ⏮ to replay it from round one (**Latest ⇥** then catches
  up). A run that starts while the page is open raises a toast with
  **Watch**; with nothing open on the Hive tab the page follows it at once.
- **Recorded runs** on the landing lists live runs first, then the rest,
  newest first, with their ending. A finished one replays like any run.
- In the player a recording swaps the run knobs for a recording picker, a
  LIVE badge while its tail is followed, and **From start**. The transcript
  shows tool calls (🐍 `run_python`, 🔎 `web_search`, 🔧 anything else; ✓ or
  ✗) and memory (🧠 ↑recalled ↓remembered) under each turn, and each seat in
  the room carries a 🔧/🧠 counter. Turns over 280 characters or four lines
  fold until **more** is clicked, and frames are folded once per animation
  frame, so recordings of thousands of long turns stay responsive.
- `?recording=<id>` reopens one.

## Compare tab

`?compare=math-panel,math-swarm&seeds=N` runs
`hive_view <first package> --compare --seeds N --packages a,b` and streams one
result per seed. The single-seat baseline is always included. The page draws
accuracy, convergence and mean rounds per package, and a per-seed strip
(green correct, red wrong, dark no decision). Clicking a cell opens that seed
and problem in the player.

## Hive API

- `GET /api/hives` returns `{root, packages: [{name, id, title, seats, desks,
  summary, kind}], live_available, live_model_default}`. Structures come from
  `hive_view <dir> --json`, cached by the package's newest file mtime.
  `live_available` is true only when the server's own environment has a
  non-empty `OPENROUTER_API_KEY` (the key is never sent);
  `live_model_default` is `OPENROUTER_MODEL` when it is a valid id, else
  `openai/gpt-oss-120b:nitro`.
- `GET /api/hives/<name>` returns that structure.
- `GET /api/hives/<name>/run?seed=&round_width=&turn_budget=&problem=` is a
  server-sent-event stream: `status`, then one `frame` per NDJSON line, then
  `done`, or `error` with `{message, stderr}` (the stderr tail) on a non-zero
  exit. Each line is written as the child prints it, with a `: keepalive`
  comment every 15 s so a quiet live run is not dropped by a proxy.
  `&live=1&model=<id>&max_calls=N` makes it a live run:
  `hive_view ... --live --model <id> --max-calls <n>`. It is a 400 unless
  `live_available`, unless the model matches
  `^[A-Za-z0-9][A-Za-z0-9._\-/]{0,99}(:[A-Za-z0-9._-]{1,30})?$` and contains
  none of `online`, `search`, `sonar` or `web` (any case, anywhere; the same
  list `hive_view --live` refuses), and unless `max_calls` is 1..300 (default
  300). A live run is stopped after 30 minutes; offline runs have no cap.
- `GET /api/compare?packages=a,b&seeds=N` streams `seed` events and a final
  `summary`, then `done` or `error`.
- `GET /api/recordings` returns `{roots, recordings: [{id, path, package,
  problem, model, seed, round, desk, turns, tool_calls, calls, started,
  frames, size, mtime, live, active, ended, ending, correct, structure}]}`,
  newest change first. `live` is no `end` frame and a change within 30 s;
  `active` the same within 10 minutes. Ids are `<root index>:<relative dir>`
  and resolve only to a `frames.ndjson` inside its root (symlinks included).
- `GET /api/recordings/events` pushes one `recordings` event with that listing
  at once and again whenever a recording appears, grows, goes quiet or ends
  (rescanned every second while a page listens).
- `GET /api/recordings/<id>/structure` returns its `structure.json`, or its
  package's structure.
- `GET /api/recordings/<id>/frames` streams `status` (`{live, active,
  ended}`), one `frame` per line already written, then, while the run is
  `active`, every line appended (polled every 500 ms; a partial last line
  waits for its newline), and `done` (`{ended}`) after the `end` frame, or
  once the file has not grown for 10 minutes.

The first request builds the binary once with
`cargo build -p tinyhivemind-lang --example hive_view` (concurrent requests
share one build) and then runs `target/debug/examples/hive_view`.
`HIVE_PACKAGES` replaces the package root. `HIVE_VIEW_BIN` replaces the binary
and skips the build. The child's environment is the server's own. A `.mjs` runs under node, so
`HIVE_VIEW_BIN=tools/fake-hive-view.mjs` replays the fixtures;
`HIVE_FAKE_DELAY_MS=3000` slows its replay to live pace.

## Security

There is no authentication. Anyone who can reach the port can read every
trace and recording under the roots (recordings carry model replies and tool
summaries) and make the server run `hive_view`, which costs CPU.
Hive runs are fenced in: every process is spawned from an argv array with no
shell, `<name>` must be a package from the listing (anything else is a 404,
`..` included), and every knob is a plain integer inside its bounds (seed
0..2^32, round width 1..64, turn budget 1..2000, seeds 1..200) or the request
is a 400. A problem id must be short and alphanumeric. The child is killed
when the client disconnects. A live run spends money on the server's
OpenRouter key, so anyone who can reach the port can spend it, up to 300
calls a run. The key reaches `hive_view` only through the environment it
inherits, never argv, a response or a log; model ids naming `online`, `search`,
`sonar` or `web` are refused so a live run cannot browse. Bind to localhost, or keep the port off untrusted
networks.

## Checks

```sh
npm test                    # node --test: server and pure page modules
npm run build               # static page (Runs tab drag and drop only)
chrome --headless=new --remote-debugging-port=9333 --user-data-dir=/somewhere &
node tools/hive-check.mjs http://localhost:8099/ shots   # SCHEME=light for light mode
```

`hive-check.mjs` opens the gallery, `?hive=review-board`, `?hive=math-swarm`
(asserting five desk badges and ending rows and at least one accepted and one
refused referral row, and shooting `math-swarm-end.png` and one desk's
transcript tab)
and `?compare=math-panel,math-swarm&seeds=10`, waits for each ending, replays
a seed from the compare strip, and writes PNGs to `shots/` (git-ignored). It
also opens the `coin-panel` fixture through `?recording=` and asserts its
tool-call, memory, seat-counter and fold badges (`recording.png`). With
`FOLLOW_LIVE=1` it then waits for a run in the Live now strip (start one with
`hive_view <math-swarm> --run --live --max-calls 80` from a terminal), follows
it, checks the player advances, and shoots `live-strip.png`, `live-mid.png`
and `live-end.png`.
