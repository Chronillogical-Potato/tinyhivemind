# `tinyhivemind`

The session runtime defines the host's log and selection ports. It projects
attributed conversation history, validates speech, and provides bounded
reads for threads, pins, and search.

See the [repository root README](../../README.md) for what `tinyhivemind` is
and why it exists. `tinyhivemind-core` answers what can be decided from
supplied desk and roster data. This crate handles operations that wait on a
host: paging through its session log, building a turn briefing, optionally
asking a selector, and dispatching a mention or referral through a queue.
The responder and dispatch paths remain public APIs; the completion driver
uses the session and speech types from this crate for its own episode loop.

`tinyhivemind` depends on `tinyhivemind-core` and re-exports its entire public
surface (`pub use tinyhivemind_core::*;` in [`src/lib.rs`](src/lib.rs)), so a
host takes one dependency rather than two, and the types a host handles are
the same types, not structural twins.

Crate-level docs, the runnable example, and the full module list live in
[`src/lib.rs`](src/lib.rs). Feature-module documentation is indexed in
[`src/README.md`](src/README.md); the example harnesses are indexed in
[`examples/README.md`](examples/README.md).

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
