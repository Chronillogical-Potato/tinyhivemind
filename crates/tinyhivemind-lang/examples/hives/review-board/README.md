# review-board

The minimal hive: one desk, three seats, and a quorum. An author proposes that
a change merges, two reviewers deliberate blind and then in the open, and the
room converges on `#merge` once two grounded supporters carry it.

| Path | Purpose |
| --- | --- |
| `hive.json` | One desk, three seats, a blind opening round and grounded quorum of two |
| `constitution.json` | Default caps: 60 turns, width 8 |
| `memory.json` | No memory |
| `roles/` | Author and reviewer charters |
| `seats/` | One prompt per seat |
| `script.json` | The scripted lines `hive_view --run` replays (not part of the package) |

`script.json` is neither canonical nor Markdown, so `parse` drops it; only the
`hive_view` example reads it.
