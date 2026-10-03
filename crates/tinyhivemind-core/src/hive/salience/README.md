# Salience

Recency, importance and relevance, folded into one comparable score. This is
the whole attention-and-decay layer, in one function each trace is scored
through.

## Design

`salience(trace, at, weights, relevance)` computes the retrieval score from
the memory-stream architecture: a weighted sum of an exponentially decaying
recency term, a standing importance term keyed to the trace's kind, and a
caller-supplied topical relevance term. `SalienceWeights::DEFAULT` reproduces
the weights that implementation's *code* actually used (`0.5`, `3.0`, `2.0`),
not the ones its paper states, and decays by rank in sequence distance rather
than by elapsed wall-clock time — the right substitution for a transcript that
carries no clock.

The function is split in two because the attention market amortizes it:

- [`standing`] computes recency and importance — properties of the trace
  alone — once per trace.
- [`with_relevance`] adds one reader's topical match on top, once per member
  per trace.

`salience` composes the two for a caller that only ever has one reader in
mind; `bids` in the parent [`attention`] module calls them separately because
it has many.

[`attention`]: crate::attention

### Decay is not optional

Without decay, whoever spoke first keeps the floor forever — the same failure
ant trails avoid only because pheromone evaporates. [`decay`] halves a fixed
`SCALE` once per `half_life` of sequence distance, using an integer shift for
the whole halvings and a linear interpolation within the final one, and it is
shared verbatim with the directory fold's deposit decay: two decay curves in
one crate would be two things to tune, and they would drift apart.

### Fixed point throughout

Weights are tenths (`SalienceWeights`), and every score is thousandths
(`Salience`, `SCALE`). No floating point appears anywhere in this module, so
every score is `Eq` and every fold reproduces exactly — the same discipline
the whole crate holds to, stated in `CLAUDE.md`.

## Public surface

| Item | Purpose |
| --- | --- |
| `salience` | The whole fold: recency, importance and relevance for one trace, one reader. |
| `Salience` | A fixed-point score in thousandths, wrapping `i64`. |
| `SalienceWeights` | Tenths weights on the three terms, plus the recency half-life. `DEFAULT` reproduces the memory-stream reference implementation. |
| `importance` | Standing importance of a trace kind alone, in thousandths — ordinal, not measured; see its rustdoc for the ranking and why. |

`standing`, `with_relevance` and `decay` are `pub(crate)`: they exist so the
attention market can amortize the reader-independent half of the score across
every member, and are not meant to be called from outside this crate.

## Operational constraints

- **`half_life` must not be zero.** `salience` and `standing` both return
  [`Error::ZeroHalfLife`][error] rather than dividing by it.
- **`relevance` saturates at 100.** A caller passing a value above the
  declared `0..=100` range does not get an amplified score.
- **`decay` saturates at zero past ~63 half-lives** of sequence distance,
  rather than overflowing the bit shift.

[error]: crate::error::Error::ZeroHalfLife
