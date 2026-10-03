# tinyhivemind-core

The host-neutral TinyHiveMind library lives here. Its top-level modules handle
desks, rosters, mentions, approval, and responder decisions. `runtime` adds
session projection and host ports; `hive` adds task division and deliberation;
`embed` and `typesafe` handle conversation routing and System One questions;
`driver` coordinates completion episodes over host-bound agent handles.

The crate opens no storage, socket, or model client and depends on no harness.
The caller supplies snapshots and implements the narrow waiting ports.
[`src/lib.rs`](src/lib.rs) defines the public modules, and
[`src/README.md`](src/README.md) indexes them. The
[workspace dependency map](../../docs/crate-dependencies.md) shows the other
two crates.
