# `render`

`tool_specs()` as MCP tool definitions, and MCP arguments onto `CallArguments`.

| file | holds |
| --- | --- |
| `mod.rs` | `served()`, `serves()`, `tool_definitions()`, `Arguments`, `arguments()` |
| `test.rs` | the served set, verbatim descriptions, the two thread arguments, every argument shape a dispatcher sends |

Descriptions are the specs' own words. The only additions are `chat` and
`parent` on every tool, and the seat list as the choices `ask` and
`ask_teammates` offer. `dm` is withheld from every host; a host withholds more
by name through `EpisodeTools::withhold`.
