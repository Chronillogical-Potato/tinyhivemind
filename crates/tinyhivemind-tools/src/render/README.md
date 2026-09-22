# `render`

`tool_specs()` as MCP tool definitions, and MCP arguments onto `CallArguments`.

| file | holds |
| --- | --- |
| `mod.rs` | `served()`, `serves()`, `tool_definitions()`, `Arguments`, `arguments()` |
| `test.rs` | the served set, verbatim descriptions, the two thread arguments, every argument shape a dispatcher sends |

Descriptions are the specs' own words. The only additions are `chat` and
`parent` on every tool, and the seat list as `ask`'s choices. `dm` is withheld.
