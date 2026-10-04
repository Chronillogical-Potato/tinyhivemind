# sandbox

| File | Purpose |
| --- | --- |
| `mod.rs` | `Exec` trait, `refuse` command policy, `truncate` |
| `docker.rs` | `DockerExec`: `docker exec` with an in-container `timeout` and a host deadline |
| `rpc.rs` | `StdioExec`: JSON-lines requests on stdout, replies demultiplexed by id from stdin |
| `test.rs` | policy, truncation and RPC framing with an in-process peer |
