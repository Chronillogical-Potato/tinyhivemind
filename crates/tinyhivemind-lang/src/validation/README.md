# validation

Checks references, paths, schema version, bounded policies and constitution
memory restrictions before activation. Desk graphs may contain cycles; goal
decompositions may not.

Every rule pushes its finding with the file and JSON Pointer it concerns.
`validate` returns the first as an `Error`; `diagnose` returns every one as a
`Diagnostic`. Rules run in one fixed order, so the first finding is the error
fail-fast validation stopped at. A seat's role is reported at
`hive.json:/seats/<i>/role`, a missing prompt at the manifest field naming it,
frontmatter and context markers on the Markdown file, memory findings in
`memory.json`, and protected namespaces and read-only pins in
`constitution.json`.

| File | Purpose |
| --- | --- |
| `mod.rs` | `validate`, `diagnose`, `validate_path`, `validate_namespace`, and the manifest rules |
| `bounds.rs` | Policy and coordinator bounds against the constitution's caps |
| `memory.rs` | Memory identities, bindings and reaches |
| `test/` | Fail-fast regressions and finding locations; see [`test/README.md`](test/README.md) |
