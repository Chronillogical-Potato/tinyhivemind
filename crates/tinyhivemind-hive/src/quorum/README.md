# Quorum and cross-inhibition

Quorum is what turns a pile of traces into a decision: `standings` folds a
transcript's `!propose`/`!support`/`!object`/`!refute` traces into one
`TopicStanding` per topic, and `consensus` reads those standings to say
whether the room is still deliberating, has settled on exactly one topic, or
has tied two or more.

## Design

Two mechanisms here come from how honeybee swarms actually settle on a nest
site rather than from voting theory, and both are load-bearing.

**Quorum is local and weighted.** A topic carries when its admitted expected
support reaches `threshold × 1_000_000` within the last `window` sequences.
The compatibility fold gives each distinct supporter one full unit; the typed
fold uses each member's latest Choice probability multiplied by normalized
evidence Score after its Noul violation gate. The result is order-independent and idempotent, so a
participant that catches up late folds to the same standing as one that
watched live. `test/fold_discipline.rs` is the regression suite for that
property.

**Cross-inhibition targets the advocate, not the option.** An `!object`
naming a message removes that message's author from the supporter set of
every topic they were advocating there. Subtracting from a score cannot break
a tie between two equally supported options; silencing an advocate can, and
that asymmetry is the entire reason the mechanism is shaped this way. See
`test/cross_inhibition.rs`.

A grounded `!refute #topic ^N` is recorded in the standing for audit. It does
not change support or consensus. The optional refutation cap and citation-chain
gate were retired after the measured trials in
`docs/experiments/2026-09-01-refutation-and-grounds.md`.

## Public surface

| Item | Purpose |
| --- | --- |
| `standings` | Fold traces into one `TopicStanding` per topic, at a given sequence. |
| `standings_with_evaluations` | Replace full-unit support with source-bound, admitted fixed-point evaluations. |
| `consensus` | Read standings for `Deliberating` \| `Quorum` \| `Deadlocked`. |
| `QuorumPolicy` | Threshold, window, and `require_grounded`. |
| `TopicStanding` | Supporters, silenced advocates, refuters, salience weight, and expected probability support. |
| `ConsensusState` | What the standings add up to. |

`standings` and `consensus` are pure folds over a caller-supplied `&[Trace]`
and `&QuorumPolicy`; neither reads a clock or a store, and both are used by
`episode::step` at the episode horizon.

## File layout

`mod.rs` holds `standings`, `consensus`, and the private folds between them
(`silenced_advocates` and `refutations`); `types.rs` holds the stable `QuorumPolicy`,
`TopicStanding` and `ConsensusState` payloads. The unit suite lives under
`test/`, one file per behavior area:

| File | Covers |
| --- | --- |
| `test/support.rs` | Shared fixtures: `said`, the shared `policy`, `fold`/`standing`, and the deadlocked/contested transcript builders. |
| `test/wire_forms.rs` | Serde pins for the policy, standing, and tagged `ConsensusState` variants. |
| `test/support_counting.rs` | Plain support counting: proposers, distinct supporters, ungrounded support, the window, and deferral as a non-vote. |
| `test/cross_inhibition.rs` | The objection mechanism, and the proof it can break a tie a subtracted score cannot. |
| `test/fold_discipline.rs` | Order-independence, idempotence, and `carried`'s threshold check. |
| `test/probabilistic.rs` | Fixed-point stance/evidence composition, admission, latest-member replacement, freshness, and malformed distributions. |
| `test/refutation.rs` | Audit-only refutation recording and fold discipline. |

Every submodule is a descendant of `quorum`, so each can see the module's
private items exactly as the old flat `test.rs` could — nothing here changes
what a test may reach, only where it lives.

## Operational constraints

- **`threshold` and `window` must be nonzero.** A zero threshold or window
  would make the count meaningless; both are rejected as
  `Error::ZeroQuorumThreshold` / `Error::ZeroQuorumWindow` rather than
  silently folding to an always-carried or always-empty standing.
- **An objection cannot silence its own author.** Otherwise an agent could
  retract a peer's support by objecting to itself.
- **A refutation attaches only to a topic some member advocated.** Refuting
  something nobody put on the floor is inert, so one member cannot
  manufacture a standing nobody else ever mentioned.
- **Nothing here uses floating point.** Every score is fixed-point integer, so
  the fold is reproducible and every payload derives `Eq`.
