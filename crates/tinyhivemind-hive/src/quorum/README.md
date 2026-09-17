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

Two more knobs sit on top of those two mechanisms, both off by default because
the benchmark scored them and they lost — see
`docs/experiments/2026-09-01-refutation-and-grounds.md`:

- **Refutation targets the option, not the advocate.** `!refute #topic ^N`
  argues cited evidence against a topic itself, rather than against any one
  advocate. `refutation_cap` caps a topic out of contention once enough
  distinct grounded refuters have named it; it never silences anybody or
  removes a supporter, because `carried` reads a supporter *count* and
  subtracting from `support` would change nothing a cap doesn't already say
  more directly. See `test/refutation.rs`.
- **Grounds are weighed, not counted.** Under `require_evidential`, a support
  counts only if its citation chain — followed transitively, inside the
  window only — reaches a `TraceKind::Evidence`. A support citing another
  support is a citation of an opinion, which is exactly the condition under
  which an information cascade forms. See `test/evidential_grounding.rs`.

## Public surface

| Item | Purpose |
| --- | --- |
| `standings` | Fold traces into one `TopicStanding` per topic, at a given sequence. |
| `standings_with_evaluations` | Replace full-unit support with source-bound, admitted fixed-point evaluations. |
| `consensus` | Read standings for `Deliberating` \| `Quorum` \| `Deadlocked`. |
| `QuorumPolicy` | Threshold, window, `require_grounded`, `refutation_cap`, `require_evidential`. |
| `TopicStanding` | Supporters, silenced advocates, refuters, salience weight, and expected probability support. |
| `ConsensusState` | What the standings add up to. |

`standings` and `consensus` are pure folds over a caller-supplied `&[Trace]`
and `&QuorumPolicy`; neither reads a clock or a store, and both are used by
`episode::step` at the same folded sequence the directory is read at.

## File layout

`mod.rs` holds `standings`, `consensus`, and the private folds between them
(`silenced_advocates`, `refuter_pairs`, `refutations`, `reaches_evidence`, and
their supporting indexes); `types.rs` holds the stable `QuorumPolicy`,
`TopicStanding` and `ConsensusState` payloads. The unit suite lives under
`test/`, one file per behavior area:

| File | Covers |
| --- | --- |
| `test/support.rs` | Shared fixtures: `said`, the refutation-enabled `policy`, `fold`/`standing`, and the deadlocked/contested transcript builders. |
| `test/wire_forms.rs` | Serde pins for the policy, standing, and tagged `ConsensusState` variants. |
| `test/support_counting.rs` | Plain support counting: proposers, distinct supporters, ungrounded support, the window, and deferral as a non-vote. |
| `test/cross_inhibition.rs` | The objection mechanism, and the proof it can break a tie a subtracted score cannot. |
| `test/fold_discipline.rs` | Order-independence, idempotence, and `carried`'s threshold check. |
| `test/probabilistic.rs` | Fixed-point stance/evidence composition, admission, latest-member replacement, freshness, and malformed distributions. |
| `test/refutation.rs` | The refutation cap taking a topic out of contention without silencing anyone. |
| `test/evidential_grounding.rs` | `require_evidential`'s citation-chain gate, including its cycle and window limits. |

Every submodule is a descendant of `quorum`, so each can see the module's
private items exactly as the old flat `test.rs` could — nothing here changes
what a test may reach, only where it lives.

## Operational constraints

- **`threshold` and `window` must be nonzero.** A zero threshold or window
  would make the count meaningless; both are rejected as
  `Error::ZeroQuorumThreshold` / `Error::ZeroQuorumWindow` rather than
  silently folding to an always-carried or always-empty standing.
- **`refutation_cap: Some(0)` is rejected, not read as "off".** `None` is the
  off state; `Some(0)` would cap a topic before anyone could refute it, which
  is a configuration error rather than a quieter way to disable the mechanism.
- **An objection cannot silence its own author.** Otherwise an agent could
  retract a peer's support by objecting to itself.
- **A refutation attaches only to a topic some member advocated.** Refuting
  something nobody put on the floor is inert, so one member cannot
  manufacture a standing nobody else ever mentioned.
- **Nothing here uses floating point.** Every score is fixed-point integer, so
  the fold is reproducible and every payload derives `Eq`.
