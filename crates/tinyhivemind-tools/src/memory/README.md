# `memory`

The hive's memory tools, served over a host's `WorkingMemory`.

| file | holds |
| --- | --- |
| `mod.rs` | `memory_tool_definitions()` (native specs), `MemoryTools` and its async `call(seat, name, args)` |
| `test.rs` | per-seat attribution, scope privacy, refusals, and that an engine failure never leaks its message |

The hive owns the tool names, schemas and the words a seat is answered in.
The host owns the engine: it implements `tinyhivemind_core::runtime::WorkingMemory`
and passes it to `MemoryTools::new`. The seat id comes from the host, never from
the call. A harness wraps each definition natively and calls `MemoryTools::call`
from its execute method.
