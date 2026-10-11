# research-debate

A goal tree, missions, and a shared context document. The desk decides whether
to cache the embedding index; the verdict goal decomposes into a survey and a
critique, and each mission binds a role to the goals it owns. The librarian and
skeptic read `context/brief.md`, whose `<!-- item:id -->` markers make each
paragraph separately patchable. The desk picks responders with
`responder_mode: auto`.

| Path | Purpose |
| --- | --- |
| `hive.json` | Goals with subgoals, missions, one context document, an auto desk |
| `constitution.json` | Default caps |
| `memory.json` | No memory |
| `context/brief.md` | The itemized brief every seat may cite |
| `roles/`, `seats/` | Charters and prompts |
