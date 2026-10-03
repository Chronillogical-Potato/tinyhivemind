# Quorum unit tests

Unit tests for quorum counting and cross-inhibition, grouped by the behavior
each file exercises. [`support.rs`](support.rs) holds the fixtures every other
file shares.

| file | covers |
| --- | --- |
| [`mod.rs`](mod.rs) | Wires the submodules together; carries no tests of its own. |
| [`support.rs`](support.rs) | Shared fixtures: transcript builders, the shared `policy`, and the `fold`/`standing` helpers that turn a transcript into the one standing a test cares about. |
| [`support_counting.rs`](support_counting.rs) | Plain support counting: who a topic's supporter set includes, what a malformed policy rejects, and that a trace naming no topic/agent (or a deferral) contributes nothing. |
| [`fold_discipline.rs`](fold_discipline.rs) | `standings` is order-independent and redelivery-idempotent, and `carried` reads the supporter count exactly as `QuorumPolicy::threshold` says. |
| [`cross_inhibition.rs`](cross_inhibition.rs) | The mechanism that silences an advocate rather than debiting an option, and the proof it can break a tie that a subtracted score never could. |
| [`refutation.rs`](refutation.rs) | Audit-only refutation recording and fold discipline. |
| [`wire_forms.rs`](wire_forms.rs) | Serde wire-form pins for `QuorumPolicy`, `TopicStanding`, `ConsensusState`, and the shipping default's shape. |
