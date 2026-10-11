# Hive and Compare tabs

The page half of the Hive tab: plain ES modules and SVG with no frameworks or
chart libraries, drawn from what `hive_view` streams through the dev server
(`server/hive-api.js`). The pure modules (`simulation`, `stream`'s parser,
`layout`, `compare`'s summaries) have no DOM at import time, so `npm test`
covers them under node.

| File | Purpose |
| --- | --- |
| `index.js` | Entry: tabs, URL state (`?view=hive`, `?hive=<name>&seed=N`, `?compare=a,b&seeds=N`), and the wiring from streams to panels |
| `api.js` | Fetches the listing and structures and opens the run and compare streams |
| `stream.js` | Server-sent events over `fetch`, so a 400 shows its message and a finished stream does not reconnect |
| `simulation.js` | Pure reducer: frames → the room after N rounds (lines, answers, standings carried over, crossings, ending), plus `sawBy` |
| `player.js` | The replay clock: buffers frames and steps through them at 0.5× to 8× |
| `layout.js` | Pure geometry: desk boxes in a wrapping grid, seats in their first desk, compact nodes above 16 seats |
| `graph.js` | The room in SVG: desks, topology and referral edges (animated when crossed), seats, answer chips, speech bubbles |
| `standings.js` | Support and object bars per topic with the quorum threshold line |
| `transcript.js` | The rolling transcript and the selected seat's `saw`/`of` highlight |
| `banner.js` | The ending banner |
| `controls.js` | Package, knobs, Run, Play/Pause, Step, speed and the scrubber |
| `gallery.js` | Landing cards for every package |
| `compare.js` | Compare bars, the per-seed strip, and the pure `summarize`/`outcome`/`order` |
| `dom.js` | Small DOM and SVG builders and text wrapping |
| `hive.css` | Styles for both tabs, built on the theme tokens in `index.html` |
| `simulation.test.js` | `npm test`: seek, step past the end, ending, standings carry-over, `sawBy`, crossings, SSE parsing |
| `compare.test.js` | `npm test`: summaries match the stream's own, outcome classes, baseline ordering |

## Constraints

- The frames are the wire contract of `crates/tinyhivemind-lang/examples/hive_view`
  (`--json`, `--run --frames`, `--compare`). Unknown fields are ignored, so the
  binary can add fields without breaking the page.
- Which lines a turn saw is reconstructed: the binary reports only the counts
  `saw` and `of`, so the transcript highlights the `saw` most recent lines in
  front of the turn (earlier rounds only, for a blind turn).
- Speech bubbles avoid desk headers and seat nodes. In a compact hive, a
  bubble that has no free spot is dropped unless its seat is hovered or
  selected.
