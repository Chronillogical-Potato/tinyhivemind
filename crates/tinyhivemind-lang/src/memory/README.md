# memory

Wire types for `memory.json` (`MemorySpec`: identities, lifecycles, and seat
read/remember bindings) and `constitution.json` (`Constitution`: host-pinned
caps, permitted edit classes, protected namespaces, read-only pins, canaries,
and opaque evaluator settings). Stored entries remain owned by the host memory
engine.

| File | Purpose |
| --- | --- |
| `mod.rs` | Module documentation and re-exports. |
| `types.rs` | `Constitution`, `MemorySpec`, `MemoryIdentity`, `SeatMemory`, `MemoryReach`, and their enums. |

Read reaches use explicit namespaces within the package root. `inherit` must
be false because engine ancestor inheritance includes the global root; list
permitted ancestors as separate reaches instead. Hosts enforce read-only,
recall budgets, remembered categories and fork lifecycle from lowering.
Validation of these types lives in `../validation`.
