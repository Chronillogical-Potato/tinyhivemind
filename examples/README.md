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
| [`openhuman/`](openhuman/README.md) | Prove embedded routing and provide live completion-hive and hermetic DeepSWE binaries. | `cargo run --manifest-path examples/openhuman/Cargo.toml` |

Each directory is its own Cargo workspace. That boundary is load-bearing:
normal TinyHiveMind builds, purity checks, and downstream path dependencies do
not resolve the integration-only packages declared here.
