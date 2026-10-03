# Episode unit tests

Unit tests for the episode state machine, grouped by the behavior each file
exercises. [`support.rs`](support.rs) holds the fixtures every other file
shares; the rest follow the seams `step`'s own implementation is split along.

| file | covers |
| --- | --- |
| [`mod.rs`](mod.rs) | Wires the submodules together; carries no tests of its own. |
| [`support.rs`](support.rs) | Shared fixtures: a three-member `Room`, transcript builders (a message, an aside, an operator notice), and the `run`/`speaking` helpers most tests drive `step` through. |
| [`turn_dynamics.rs`](turn_dynamics.rs) | Per-turn attention dynamics: blind visibility during the opening round, and the threshold charge that rotates the floor. |
| [`quorum_and_convergence.rs`](quorum_and_convergence.rs) | The one-way `Deliberate` → `Commit` phase flip, and that convergence requires a `!commit` naming the carried topic strictly after that boundary. |
| [`deadlock.rs`](deadlock.rs) | Deadlock, and cross-inhibition through the whole machine: terminal only once nobody has backed neither carried topic. |
| [`termination.rs`](termination.rs) | Sequential-round fixtures and budget-bounded termination. |
| [`concurrent_asides.rs`](concurrent_asides.rs) | A turn-holder's aside riding alongside its desk-visible move — free to the room, but its consumed sequence can still shift a later row across a quorum window. |
| [`off_floor_asides.rs`](off_floor_asides.rs) | Asides carry information, never support: no supporter is added, and a non-member sees a stub where a member sees the content. |
| [`failure_paths.rs`](failure_paths.rs) | Validation: a malformed roster, desk snapshot, or policy is rejected before authorizing a turn; a threshold naming a non-member is rejected like a retired member is dropped. |
| [`wire_forms.rs`](wire_forms.rs) | Serde wire-form pins for `EpisodePolicy`, `EpisodeState`, the tagged `HiveStep` variants, and the shipping default's budget. |
