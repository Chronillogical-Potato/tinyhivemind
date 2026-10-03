# `raw`

These helpers support hosts that build OpenHuman core sessions themselves.
`LibraryHost` boots the core with a caller-owned provider route and scopes a
turn under that context. `register_seats` writes the definitions the hosted
turn needs before OpenHuman reads its process-wide registry. The native tool
belt wraps `EpisodeTools::call` for hosts and embedded agents.

| File | Purpose |
| --- | --- |
| `mod.rs` | `Route`, `register_seats`, and exports. |
| `library.rs` | `LibraryHost` setup, session builder, and scope. |
| `tools.rs` | Native tools backed by the shared episode call record. |
| `policy.rs` | Memory that retains nothing between seeded turns. |
| `test.rs` | Tool, memory, route, and registry behavior. |
