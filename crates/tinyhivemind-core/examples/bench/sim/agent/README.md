# Sim agent

The simulated participant's type and behavior, split by concern rather than
one large `impl SimAgent` block. See [`../README.md`](../README.md) for the
task it participates in and [`../../README.md`](../../README.md) for what its
turns are scored against.

| file | what it does |
| --- | --- |
| [`mod.rs`](mod.rs) | Defines `SimAgent` itself, so the rest of `sim` has one place to name the type; wires the two descendant modules below. |
| [`state.rs`](state.rs) | Construction and mutation: building a member, folding an outside reading into it, narrowing the window it reads through — the bookkeeping `turn.rs` reads back via `score`/`favourite`. Decides nothing about what a turn says. |
| [`turn.rs`](turn.rs) | What one turn says: the pairwise check, the evidence-first opening, and the ladder of floor moves `compose` works down. Reads a member's holdings through `state.rs`'s accessors; mutates them only through taking a reading or spending a check. Also the `Participant` impl that lets the host drive it. |
