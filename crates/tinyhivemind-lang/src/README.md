# Language source

All operations consume caller-owned values. No module opens storage or starts
an agent. `lib.rs` carries the crate overview and runnable example, and
re-exports the module surface: `parse`, `validate`, `validate_path`,
`validate_namespace`, `lower`, and every wire type. `patch` and `lineage`
keep their own `Error` and `Result`, so they are used through their module
paths.

| Module | Purpose |
| --- | --- |
| `document` | `Package` and `Manifest` wire types, and `parse` over a caller-supplied document map. Its tests live in `validation/test.rs`. |
| `memory` | `Constitution` and `MemorySpec`: caps, protected namespaces, identities, lifecycles, reaches and recall settings. |
| `validation` | `validate`, `validate_path`, `validate_namespace`: references, paths, frontmatter agreement, goal acyclicity, policy bounds and memory access. |
| `lowering` | `lower`: core desks, routing candidates and host seat creation inputs. |
| `error` | The package-level `Error` returned by parsing, validation and lowering. |
| `patch` | Classified editor proposals: `apply`, `guard`, and `diff`. |
| `lineage` | Content identities, evaluated records, `accepted_head` and `pareto_frontier`. |
