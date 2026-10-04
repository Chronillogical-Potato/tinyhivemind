# Run viewer

A single-page, dependency-free viewer for the telemetry a hive-lab run emits
(`TraceEvent` in `crates/tinyhivemind-core/src/telemetry/types.rs`, stamped with
`run`, `seq`, `at_ms`). It works offline, follows the system light/dark setting,
and stays usable at 360px (charts scroll horizontally inside their card).

| File | Purpose |
| --- | --- |
| `viewer.html` | Page shell, styles, and theme tokens |
| `viewer.js` | Parsing, analysis, SVG rendering (no libraries) |
| `fixtures/sample.jsonl` | Two demo runs: `hive-4seat` (concurrent) and `baseline-serial` |

## Use

- Open `viewer.html` and drop one or more `.jsonl` files, or use *Choose files*.
- Several runs may share one file (grouped by the `run` field) or come from
  several files. Axes are shared across loaded runs so they compare fairly.
- `?file=a.jsonl&file=b.jsonl` fetches files relative to the page. Fetching
  needs an http server (for example the docker static server), not `file://`.
  *Load sample* uses the same path.
- Quick local try: `python3 -m http.server -d examples/lab/viewer` then open
  `http://localhost:8000/viewer.html?file=fixtures/sample.jsonl`.

## Views

1. **Waterfall**: a lane per seat. A `turn_started` is matched to its
   `turn_finished` by the `turn` id (seat order when the id is absent). Overlapping turns of one seat
   stack. A finish with no start is reconstructed from `latency_ms`; a start
   with no finish runs to the end of the run (dashed). Tool calls sit in their
   turn's row (by `turn`), spanning `[at_ms - latency_ms, at_ms]`; refused calls
   are red with a cross and the `reason` in the tooltip.
2. **Timeline**: rounds (phase and visibility; dashed = blind), one row per
   conductor kind, outcome markers (C converged, D deadlocked, E exhausted,
   I idle), marks (triangle) and checkpoints (square).
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
