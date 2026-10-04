# Snapshot storage

| File | Responsibility |
| --- | --- |
| `mod.rs` | Object-safe `Storage` port and memory implementation |
| `types.rs` | Durable agents, transcripts, queues, running turns and conductor state |
| `sqlite.rs` | Default-feature version-one SQLite snapshot implementation |
| `test.rs` | Shared CAS contract, atomic failure, reopen and schema validation |

`commit(expected_revision, next)` advances exactly one revision or changes
nothing. SQLite checks the revision and writes its JSON snapshot in one immediate
transaction. Its revision is stored as text to preserve the full unsigned range.
Snapshots contain no agent handles, closures, credentials, or runtime singleton.

Storage is replaceable by the host. A newly loaded coordinator records running
reservations as interrupted and requires supplied handles to be reattached before
unstarted jobs run. Durable session IDs remain bound across process runtimes.
