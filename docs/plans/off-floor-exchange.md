# Implement off-floor exchange

> **Path note.** Paths and commands below are updated to the consolidated
> `crates/tinyhivemind-core` layout; this plan originally targeted a separate
> `tinyhivemind-hive` crate, since folded into core's `hive` module.

Linked specification: [`../specs/off-floor-exchange.md`](../specs/off-floor-exchange.md)

## Goal

Let members exchange privately **without any of them taking the floor**, so the
volume of exchange is set by what a host will pay for rather than by how many
turns the deliberation happens to take — while keeping the episode's decision
provably identical whether an exchange happened or not, and keeping the price
visible rather than folded into an existing column.

No new port, no waiting, no second journal, and no change to `step`.

## Test-first tasks

1. Add failing invariants for the property the whole design rests on, *before*
   the mechanism exists: `step` returns the same `HiveStep`, including the state
   it commits, for a transcript with and without private rows interleaved
   anywhere. One readable case in `crates/tinyhivemind-core/src/hive/episode/test/`
   and one *addition* invariant over arbitrary transcripts in
   `crates/tinyhivemind-core/tests/migrated_hive/fuzz_invariants.rs`, whose corpus carries the
   same fuzzed grammar in its private rows as in its desk rows. Confirm both go
   red when the audience filter is removed from `live_traces`.
2. Add failing unit tests for the fold's decisions, then implement
   `crates/tinyhivemind-core/src/hive/exchange/`: `ExchangePolicy` off by default,
   `ExchangeRound`, `NoExchangeReason`, and `exchange`. Cover the default
   opening no round, a zero cap closing rather than erroring, spend read back
   out of the transcript, a spent member dropping out, each ceiling binding
   independently, rows at or below the watermark not counting, a non-member's
   rows charged to nobody, a desk of one, and the wire forms.
3. Add a failing test that `remaining` is clamped **per member** before summing
   rather than in aggregate, and implement it. The two disagree only when spend
   is uneven, which is exactly when a host sizing a batch off the field would
   overallocate.
4. Add a failing integration invariant driving rounds to exhaustion across a
   grid of policies: it terminates, never exceeds
   `round_cap × max_active_members`, closes with the reason the
   binding cap implies, and leaves `step` unchanged.
5. Export `ExchangePolicy`, `ExchangeRound`, `NoExchangeReason` and `exchange`
   from `crates/tinyhivemind-core/src/hive/mod.rs`, and add the module `README.md`.
6. Wire the harness: `AsideMode::OffFloor`, `Participant::exchange`, one round
   between turns, and `--exchange-cap` as a knob separate from `--aside-cap`
   because it bounds model calls rather than the room's turns.
7. Add the `calls/ep` column, counting members *asked* rather than rows written
   because a declined round costs the same call as a productive one, and kept
   out of `cost/ep` — which is defined as each
   speaker's own cost times its turns and is asserted to be exactly that — so
   the price of an exchange cannot hide inside an existing number.
8. Add the silence controls `hive+quiet` and `hive+hush`, which write the
   identical rows on the identical schedule and discard every answer. A private
   row consumes a sequence number and salience decays over raw sequence
   distance, so without these the arms cannot distinguish information from
   perturbation.
9. Record the measurement, including a loss if it is one, in
   `docs/experiments/2026-09-07-why-asides-lose.md`, and write ADR 0012.

## Verification

```sh
cargo fmt --all -- --check
cargo clippy --all-targets --all-features -- -D warnings
cargo build --all-targets --all-features
cargo test --all-features
.github/scripts/assert-pure.sh
RUSTDOCFLAGS="-D warnings" cargo doc --no-deps --all-features
cargo run --release -p tinyhivemind-core --example bench -- --stats-check
```

Plus the identity check, which is what makes every comparison a difference in
one thing rather than in two:

```sh
cargo run --release -p tinyhivemind-core --example bench -- \
  --hidden-profile --blind-evidence --budget 40 --episodes 500 \
  --aside-cap 0 --exchange-cap 0
```

Every aside arm must report `+0.0 [+0.0, +0.0]` against `hive+`.

## Completion checklist

- [x] Specification accepted before implementation.
- [x] `step` invariance under added private rows pinned by a readable case and
      by a fuzz invariant, both confirmed to fail without the audience filter.
- [x] Fold decisions, both ceilings, watermark scoping and wire forms tested.
- [x] `remaining` clamped per member, with the aggregate form pinned as a
      regression.
- [x] Round exhaustion terminates inside the policy's computed worst case.
- [x] Module `README.md` written.
- [x] Price displayed in a column of its own rather than folded into `cost/ep`.
- [x] Silence controls run, so a gain is information rather than perturbation.
- [x] Measurement recorded, with the ceiling it is measured against.
- [x] `ExchangePolicy::DEFAULT` disabled, and a disabled policy reproduces every
      published number to the decimal.
