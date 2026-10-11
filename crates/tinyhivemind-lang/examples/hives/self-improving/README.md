# self-improving

The package the patch and lineage walk-through edits. Its constitution permits
prompt, context and goal edits and nothing else, so an editor can rewrite a
seat prompt or a playbook item but cannot add a seat or loosen a policy. The
held-out evaluation namespace is protected, a contamination canary guards
learning deltas, and the evaluation ledger is a read-only pool.

| Path | Purpose |
| --- | --- |
| `hive.json` | Solver, critic and editor on one desk, with a patchable playbook |
| `constitution.json` | `permitted_edits`, canaries, protected namespaces, evaluator settings |
| `memory.json` | The solver's notes and a read-only ledger pool |
| `context/playbook.md` | Itemized playbook an editor may patch item by item |
| `roles/`, `seats/` | Charters and prompts |
