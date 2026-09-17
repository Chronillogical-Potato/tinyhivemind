# Standalone examples

These examples demonstrate integrations whose compiler or dependency graph is
deliberately outside TinyHiveMind's Rust 1.88 library workspace.

| Example | Purpose | Run |
| --- | --- | --- |
| [`openhuman/`](openhuman/README.md) | Route once through the real `JevRouter`, then run the selected seat through a minimal embedded OpenHuman agent against loopback fixtures. | `cargo run --manifest-path examples/openhuman/Cargo.toml` |

Each directory is its own Cargo workspace. That boundary is load-bearing:
normal TinyHiveMind builds, purity checks, MSRV checks, and downstream path
dependencies never resolve these integration-only packages.
