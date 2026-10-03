# `tinyhivemind`

The session runtime defines a host log port, projects attributed conversation
history, validates speech, and provides bounded reads for threads and pins.

See the [repository root README](../../README.md) for what `tinyhivemind` is
and why it exists. `tinyhivemind-core` answers what can be decided from
supplied desk and roster data. This crate handles operations that wait on a
host: paging through its session log, building a turn briefing, and asking a
digester to compact older history. The completion driver uses this crate's
session and speech types for its episode loop. A host can bind core routing
decisions to its own queues and model clients.

`tinyhivemind` depends on `tinyhivemind-core` and re-exports its entire public
surface (`pub use tinyhivemind_core::*;` in [`src/lib.rs`](src/lib.rs)), so a
host takes one dependency rather than two, and the types a host handles are
the same types, not structural twins.

Crate-level docs and the full module list live in
[`src/lib.rs`](src/lib.rs). Feature-module documentation is indexed in
[`src/README.md`](src/README.md). The runnable host integration lives in the
[OpenHuman example](../../examples/openhuman/README.md).

## How it relates to the other crates

This crate depends directly on
[`tinyhivemind-core`](../tinyhivemind-core/README.md). It uses core's pure
decisions, adds host ports and attributed session projection, and re-exports
core's public API. A host can therefore use the same desk and mention types
through the runtime crate.

The hive, embed, TypeSafe, driver, tools, MCP, and OpenHuman crates all list
this crate as a direct dependency. The hive reads `SessionMessage` and
`Sequence`; embed and TypeSafe share responder probabilities; the driver uses
conversations, sequences, and utterances; tools uses the speech vocabulary;
OpenHuman uses the session log and projection types. MCP lists the runtime
for wire tests that inspect a drained call. The runtime has no dependency
back on any of these crates.

See the [workspace dependency map](../../docs/crate-dependencies.md) for the
full graph.
