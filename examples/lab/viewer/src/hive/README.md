# Hive and Compare tabs

The page half of the Hive tab: plain ES modules and SVG with no frameworks or
chart libraries, drawn from what `hive_view` streams through the dev server
(`server/hive-api.js`). The pure modules (`simulation`, `stream`'s parser,
`layout`, `compare`'s summaries) have no DOM at import time, so `npm test`
covers them under node.

| File | Purpose |
| --- | --- |
| `index.js` | Entry: tabs, URL state (`?view=hive`, `?hive=<name>&seed=N`, `?recording=<id>`, `?compare=a,b&seeds=N`), the wiring from streams to panels, and opening and following recordings |
| `api.js` | Fetches the listing and structures, opens the run and compare streams, and the recording listing, structure, frames and pushed events |
| `activity.js` | Pure: a turn's tool calls and memory (`turnActivity`), per-seat sums, which recordings are live or finished, newly started runs, labels |
| `recordings.js` | The Live now strip, the Recorded runs list and the run-started toast |
| `stream.js` | Server-sent events over `fetch`, so a 400 shows its message and a finished stream does not reconnect |
| `simulation.js` | Pure reducer: frames → the room after N rounds (lines tagged with their floor, transcript entries, referrals and this step's fresh ones, desk endings, usage, standings carried over, crossings, ending), plus `sawBy` |
| `referrals.js` | Pure: the declared link a referral travelled (`edgeFor`), its wording, the per-desk transcript filter and tabs, and `deskPolicy` (a desk's own `episode`, else the package's) |
| `player.js` | The replay clock: buffers frames (one by one or in batches) and steps through them at 0.5× to 8×, or follows a live stream to its newest round |
| `layout.js` | Pure geometry: desk boxes in a wrapping grid, seats in their first desk, compact nodes above 16 seats |
| `graph.js` | The room in SVG: desks with ending badges, topology and referral edges, referral packets (accepted or refused, on a temporary edge when unlinked), seats with tool and memory counters, answer chips, speech bubbles |
| `standings.js` | Support and object bars per topic with the quorum threshold line |
| `transcript.js` | The rolling transcript with desk tabs, referral and desk-ending rows, tool and memory badges, folded long turns, and the selected seat's `saw`/`of` highlight |
| `banner.js` | The ending banner (with the answer desk), per-desk ending chips, and the live usage line |
| `controls.js` | Package, knobs, the live toggle with model and call cap, Run, Play/Pause, Step, speed, the scrubber and Latest; for a recording, its picker, LIVE badge and From start |
| `gallery.js` | Landing cards for every package |
| `compare.js` | Compare bars, the per-seed strip, and the pure `summarize`/`outcome`/`order` |
| `dom.js` | Small DOM and SVG builders and text wrapping |
| `hive.css` | Styles for both tabs, built on the theme tokens in `index.html` |
| `simulation.test.js` | `npm test`: seek, step past the end, ending, standings carry-over, `sawBy`, crossings, SSE parsing; per-desk rounds, referrals, desk endings, `end.desk`, usage, desk filter, link matching, live follow |
| `compare.test.js` | `npm test`: summaries match the stream's own, outcome classes, baseline ordering |
| `activity.test.js` | `npm test`: turn activity from counts or lists, per-seat sums, live/finished split, new-run detection, labels |

## Constraints

- The frames are the wire contract of `crates/tinyhivemind-lang/examples/hive_view`
  (`--json`, `--run --frames`, `--compare`). Unknown fields are ignored, so the
  binary can add fields without breaking the page.
- Which lines a turn saw is reconstructed: the binary reports only the counts
  `saw` and `of`, so the transcript highlights the `saw` most recent lines in
  front of the turn (earlier rounds only, for a blind turn).
- A multi-desk run is one floor per desk: `round.desk` names the floor,
  `round` stays global and floors may interleave. Messages cross desks only
  as `referral` frames, so the inferred round-to-round crossing animation is
  off for such a stream. A referral or `desk_end` belongs to the step that
  was current when it streamed; a referral's packet flies only on that step.
  Packets are moved by `requestAnimationFrame` from when each was first
  drawn, so a redraw mid-flight resumes rather than restarts (reduced motion
  draws them where they stop).
- Quorum standings belong to the current round's floor: the panel is titled
  with that desk and its threshold line is the desk's `desks[].episode.quorum_threshold`
  from `--json`, falling back to the package `episode` when the binary
  reports none.
- In live mode the player follows the stream until the user pauses, steps
  or scrubs; `usage` is shown whenever it has arrived.
- Speech bubbles avoid desk headers and seat nodes. In a compact hive, a
  bubble that has no free spot is dropped unless its seat is hovered or
  selected.
- A recording is the same frame stream read from disk. Its turns may carry
  `tool_calls` and `memory`, which the page shows and never requires. While
  its tail is open the player follows it like a live run; frames are batched
  per animation frame (`player.pushMany`) so a long recording loads in a few
  redraws. The transcript still draws at most its last 400 entries, and turns
  over 280 characters or four lines fold until expanded (the open set
  survives redraws).
- The recording listing is pushed (`/api/recordings/events`); the first
  listing announces nothing, later ones toast each run that newly appears
  live, except one this page started itself.

`index.test.js` runs the coordinator's actual run/finally and recording
ownership code with UI and stream boundaries stubbed. It pins completed-run
cleanup and the race where an old stream completes after a new one starts.
