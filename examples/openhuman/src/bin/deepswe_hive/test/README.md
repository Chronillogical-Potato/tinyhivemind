# DeepSWE security regression tests

| File | Purpose |
| --- | --- |
| `security.rs` | Exercises hostile Git/output paths, gitlink refusal, bounded staged action input and patch capture, and verified container cleanup. |
| `retry.rs` | Scripts loopback provider failures and native MCP outboxes to prove exactly-once reconciliation, bounded safe retries, timeout refusal, and atomic round commit. |
| `retry/` | Holds focused response builders used by the retry scenarios. |
