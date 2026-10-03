# Coordinator tests

| File | Responsibility |
| --- | --- |
| `mod.rs` | Fixtures, registration, visibility, shared sessions, and child asks |
| `lifecycle.rs` | Parked release, recovery, cancellation, session adoption, shutdown |
| `scheduling.rs` | Concurrent agents, membership snapshots, walls and budgets |
| `privacy.rs` | Private child reads, SQLite reopen, addressed-thread attribution |
| `failures.rs` | Boundary errors and failed-runner isolation |

Tests use scripted runner futures, barriers, and notifications. They require no
network, clock-based sleeps, or OpenHuman model calls.
