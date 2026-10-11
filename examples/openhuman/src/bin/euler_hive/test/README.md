# test

Offline tests for the Euler hive host, one module per behaviour. No network,
Docker, model or memory service is touched.

| File | Covers |
| --- | --- |
| `mod.rs` | Module wiring |
| `support.rs` | Shared fixtures: the package, scopes, a federation driver, canned actions |
| `package.rs` | Problem injection, per-seat tool gate, tool schemas, symlink refusal, federation opening |
| `sandbox.rs` | Docker argv, bounded output, spend budget and verify reserve, search refusals |
| `memory.rs` | Run-scoped namespaces, the rejoin watermark, read-only pools, wall clock |
| `routing.rs` | Broadcast router, first-turn prompt, system prompt |
| `federation.rs` | Asks, referrals and their return chains, verification, synthesis |
| `artifacts.rs` | Safe names, numbering, failed appends, closing a run with a failed write |
