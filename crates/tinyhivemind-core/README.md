# tinyhivemind-core

The pure decisions behind a shared agent room live here. Given borrowed desk
and roster snapshots, the crate resolves mentions, plans a responder, decides
whether a reply may trigger a child turn or referral, and evaluates approval
rules. It also handles asides and conversation identity.

All of those operations use data the caller already holds. There is no log
reader, executor, transport, or callback into a host. The sibling
[`tinyhivemind`](../tinyhivemind/README.md) crate adds the waiting boundaries
and re-exports this crate's public types. The purity check in
`.github/scripts/assert-pure.sh` guards the dependency graph.

Start with [`src/lib.rs`](src/lib.rs) for the public API and runnable example.
[`src/README.md`](src/README.md) maps each feature module to the question it
answers. [`examples/README.md`](examples/README.md) and
[`tests/README.md`](tests/README.md) index the runnable example and the public
contract tests.

## How it relates to the other crates

This crate has no dependency on another TinyHiveMind crate. It defines the
desk, roster, mention, approval, and responder types that the higher layers
share.

[`tinyhivemind`](../tinyhivemind/README.md) depends on core and re-exports
its public API. [`tinyhivemind-hive`](../tinyhivemind-hive/README.md) also
depends on core directly for code masking and core error conversion.
[`tinyhivemind-driver`](../tinyhivemind-driver/README.md) names core's error
type directly when it reports an invalid desk. These are direct Cargo
dependencies; other crates reach the same core types through the runtime's
re-export.

See the [workspace dependency map](../../docs/crate-dependencies.md) for all
nine crates.
