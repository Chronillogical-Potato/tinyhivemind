# `memory`

The reference `WorkingMemory` adapter for the examples: a markdown file with
compaction. It is one engine among many a host could plug in behind the same
port; the hive does not prefer it.

| File | Purpose |
| --- | --- |
| `mod.rs` | `MarkdownMemory` (`HIVE_MEMORY.md`), its `Compaction` policy, lock-file concurrency, and the `WorkingMemory` impl. |
| `test.rs` | Round-trip, ranking, dedupe, compaction, scope privacy, id stability, concurrent writers, stale locks, tolerant parsing. |

`Recent` holds notes verbatim; past `max_recent` the oldest fold into one-line
`Compacted` excerpts, exact repeats collapse, and the oldest excerpts drop. No
model is involved. Every operation is a locked read-modify-write saved by
atomic rename, because each seat's tool server is a separate process.
