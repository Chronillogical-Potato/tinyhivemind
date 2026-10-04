# Standalone examples

These standalone workspaces carry experiment-only runtime and tool features
that do not belong in normal TinyHiveMind builds. The root workspace now has a
Rust 1.96 floor because its `tinyhivemind-openhuman` adapter binds
the canonical `openhuman-embed` agent type directly; keeping these binaries
standalone still isolates their heavier live-provider, MCP, Docker, and
research dependencies. The pure core and hive crates remain covered by the
repository's dependency-purity check.

| Example | Purpose | Run |
| --- | --- | --- |
| [`hives/`](hives/README.md) | Offline reviewer handoff and one continuing agent session across three hives. | `cargo run --manifest-path examples/hives/Cargo.toml --bin one_hive` |
| [`openhuman/`](openhuman/README.md) | Run a basic offline OpenHuman hive, the integration proofs, or the live DeepSWE binaries. | `cargo run --manifest-path examples/openhuman/Cargo.toml --bin basic_hive` |

Each directory is its own Cargo workspace. That boundary is load-bearing:
normal TinyHiveMind builds, purity checks, and downstream path dependencies do
not resolve the integration-only packages declared here.
