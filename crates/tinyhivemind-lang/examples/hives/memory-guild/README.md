# memory-guild

Every part of the memory declaration. The package root is `team:guild`; the
archivist and scribe each hold private agent memory, the guild shares a `lore`
pool that lives for the hive's lifetime, a pinned `charter` pool is read-only
(the constitution pins it), and one scout forks its own memory beneath the lore
pool. Each binding sets what the seat reads, when the host recalls, its recall
budget, and which entry kinds it may remember. The archivist's prompt repeats
its seat and binding fields as JSON frontmatter, which `validate` checks
against the manifest.

| Path | Purpose |
| --- | --- |
| `hive.json` | One desk of four seats |
| `constitution.json` | Pins `charter` read-only |
| `memory.json` | Root, five identities, four bindings |
| `seats/archivist.md` | Prompt with JSON frontmatter |
| `roles/`, `seats/` | Charters and prompts |
