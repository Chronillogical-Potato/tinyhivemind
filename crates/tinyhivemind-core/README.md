# tinyhivemind-core

The pure decisions behind a shared agent room live here. Given borrowed desk
and roster snapshots, the crate resolves mentions, plans a responder, decides
whether a reply may trigger a child turn or referral, and evaluates approval
rules. It also handles asides, conversation identity, and bounded selection.

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
