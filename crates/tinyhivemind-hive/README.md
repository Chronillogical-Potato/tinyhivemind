# tinyhivemind-hive

This crate folds a transcript into bounded group decisions. It contains the
trace grammar, salience, quorum, attention bids, task division, and two kinds of
episode state. It is pure: the host supplies a transcript and owns the returned
state. The crate defines no storage or runtime port.

The usual entry point for deliberation is `step`. A `HiveStep::Speak` contains
one round of turns. The host may run those turns concurrently, up to
`EpisodePolicy::round_width` while the round is blind or
`EpisodePolicy::revealed_width` after it becomes visible. It appends every turn
before applying the round's `next_state`. A width of one gives sequential
behavior. See [ADR 0014](../../docs/adr/0014-a-round-authorizes-concurrent-turns.md)
and the [concurrent rounds spec](../../docs/specs/concurrent-rounds.md).

`division::divide` assigns independent task facets to seats. The
`completion` module tracks explicit completion and newly assigned work; it
does not infer completion from ordinary prose. These are separate uses of the
crate from quorum deliberation. The [crate docs](src/lib.rs) contain a runnable
quorum example and describe the measured tradeoffs behind the defaults.

The host still owns the log, model calls, scheduling, and durable state. Every
score is fixed-point integer arithmetic so a given input folds to the same
result. The module index is in [`src/README.md`](src/README.md), and the
scripted episode and benchmark are indexed in
[`examples/README.md`](examples/README.md).
