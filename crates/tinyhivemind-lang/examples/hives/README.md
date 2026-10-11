# Example hive packages

A gallery of complete packages in directory form. Between them they use every
part of the language; each one's `README.md` opens with what it demonstrates.
`tests/gallery_packages.rs` parses and lowers every one, and
[`hive_view`](../hive_view/README.md) renders and runs them offline.

| Package | Seats | Desks | What it shows |
| --- | --- | --- | --- |
| `review-board/` | 3 | 1 | The minimal hive: blind round, grounded quorum, a replayed `script.json` |
| `research-debate/` | 4 | 1 | Goals with subgoals, missions, an itemized `context/` document, `responder_mode: auto` |
| `incident-response/` | 6 | 2 | Topology, referrals with a hop limit, an approval policy, private asides |
| `memory-guild/` | 4 | 1 | Agent, pool, forked, hive-lifetime and read-only memories; bindings; JSON frontmatter |
| `self-improving/` | 3 | 1 | A constitution with permitted edits, canaries and protected namespaces |
| `math-panel/` | 5 | 1 | Five unreliable seats solving exact problems (`sim.json`) |
| `math-swarm/` | 50 | 5 | Fifty seats in five linked desks on the same problems, rounds of ten |

`sim.json` and `script.json` are neither canonical nor Markdown, so `parse`
drops them: they belong to the example, not to the package. The `README.md`
files are accepted as unreferenced Markdown documents.
