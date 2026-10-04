# Coordinator source

| File or directory | Responsibility |
| --- | --- |
| `lib.rs` | Public exports and minimal usage example |
| `error.rs` | Typed validation, persistence, and conductor failures |
| `coordinator/` | Dynamic registration, messaging, and scheduling |
| `storage/` | Serializable snapshots and transactional storage implementations |

OpenHuman types belong in the adapter crate. Core algebra remains pure.
