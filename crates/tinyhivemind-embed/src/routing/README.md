# `routing`

Jev-first semantic routing for unaddressed desk messages.

| File | Purpose |
| --- | --- |
| `mod.rs` | Pure validation and bounded async composition. |
| `types.rs` | Stable request, evaluation, policy, and plan wire types. |
| `test.rs` | Eligibility, fallback, bypass, invitation, and wire tests. |

Known destinations bypass the router. Unaddressed desk messages receive one
primary semantic evaluation; uncertainty may receive one reasoning escalation.
Every failure ends at a caller-supplied deterministic desk fallback.

The accepted Choice maximum is the primary responder. Other eligible Choice
options strictly above 20% are invited into the same bounded opening round,
ordered by probability then desk order. Contribution Nouls remain auditable but
do not choose recipients.
