# hive_check

A compiler-style check for hive packages, for an orchestrator that edits a
package's files directly.

```sh
cargo run -p tinyhivemind-lang --example hive_check -- <original-dir> <edited-dir> [--json]
cargo run -p tinyhivemind-lang --example hive_check -- <package-dir>
cargo test -p tinyhivemind-lang --example hive_check
```

Exit status 0 means clean, 1 means at least one finding, 2 a usage or read
error. Dot-files are skipped when a directory is loaded.

| File | Purpose |
| --- | --- |
| `main.rs` | Argument parsing, directory loading, `check`/`diagnose_documents`, and rendering |
| `test.rs` | Offline tests against the gallery's `self-improving` package |
