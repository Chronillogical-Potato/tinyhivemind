# incident-response

Two desks joined by topology and referrals. The triage desk owns the incident
and refers concrete fixes to the fixers desk, which refers back when a fix
needs a decision. Referral is enabled with a hop limit of two, any production
deploy needs approval from the desk's approver, and seats may open private
asides of two members that must surface their conclusion.

| Path | Purpose |
| --- | --- |
| `hive.json` | Two desks, `triage -> fixers` topology, referrals both ways, approval, asides |
| `constitution.json` | Default caps |
| `memory.json` | No memory |
| `roles/`, `seats/` | Charters and prompts |
