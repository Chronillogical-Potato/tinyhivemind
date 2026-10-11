# One seat, a panel of five, a swarm of fifty

**Date:** 2026-10-11. **Status:** Recorded.
**Code:** `hive_view --compare --seeds 50 --table` over the `math-panel` and
`math-swarm` gallery packages (command below).
**Spec:** [hive language](../specs/hive-language.md).
**Decision:** none; this record feeds the wiki's
[Hive gallery](https://github.com/tinyhumansai/tinyhivemind/wiki/Hive-gallery)
page.

## Question

Given the same exact problems and seats drawn from the same accuracy ranges,
does a hive decide more accurately as it grows from one seat to five to
fifty? The expected ordering, written down before the run, was swarm > panel >
single seat.

## Setup

This is an offline simulation. No model is called. Each package is parsed and
lowered by `tinyhivemind-lang`, then driven by core's `hive::step` through the
`hive_view` example. Seats write scripted trace markers whose content comes
from a seeded error model.

- **Problems.** Eight small problems with exact answers, defined in each
  package's `sim.json`: two lattice-path counts, two modular power sums, a
  digit sum, multiples of 3 or 5, a square difference, and a prime count.
  Each seed picks one problem, and every package solves that same problem on
  that seed. Correct means equal to the exact computation in
  `examples/hive_view/math.rs`.
- **Error model.** Each seat draws an accuracy from its role's range with
  the seed and gets the answer right with that probability. Otherwise it
  writes the problem's classic mistake (500,000 ppm) or the answer plus one
  or minus one (250,000 ppm each). A `slice` seat that misses posts the
  answer modulo its slice instead of a wrong answer.
- **Accuracy ranges, in parts per million.** These are shared by both
  packages and were fixed before any run.

  | Role | Behaviour | Range | Packages |
  | --- | --- | --- | --- |
  | conjecturer | `guess` | 250,000 – 450,000 | panel, swarm |
  | solver / prover | `solve` | 450,000 – 750,000 | panel, swarm, baseline |
  | checker / verifier | `check` | 500,000 – 800,000 | panel, swarm |
  | lead / synthesizer | `lead` | 450,000 – 750,000 | panel, swarm |
  | computer | `slice` | 300,000 – 600,000 | swarm only |

  Swarm slices are the moduli 3, 4, 5, 7, 8, 9, 11, 13, 16, and 25.
- **Single seat.** One `solve` seat on a one-member desk with a quorum of
  one, using the `baseline` entry of `sim.json`.
- **math-panel.** One desk with a conjecturer, two solvers, a checker, and a
  lead. Blind opening round of five, revealed width five, turn budget 15,
  grounded quorum of 3 within a 15-turn window. Constitution caps of 60 turns
  and width 8.
- **math-swarm.** Five desks of ten (conjecture, compute, prove, verify,
  synthesis). Blind and revealed width ten, turn budget 150, grounded quorum
  of 26 (`EpisodePolicy::for_room(50)`) within a 150-turn window.
  Constitution caps of 200 turns and width 10. Its topology and referrals are
  declared, but `hive_view` runs a multi-desk package as one combined floor,
  so they are drawn and not routed, and the quorum is hive-wide.
- **Sample.** Seeds 1 to 50, one episode per package per seed.

## Result

> The math-swarm row below was measured on one combined floor and is
> superseded by [Update: routed desks and live runs](#update-routed-desks-and-live-runs).
> The single-seat and math-panel rows are unchanged.

```sh
cargo run --release -p tinyhivemind-lang --example hive_view -- \
  crates/tinyhivemind-lang/examples/hives/math-swarm --compare --seeds 50 --table
```

```text
50 seeds, same problem per seed for every package; correct = exact answer

package      seats accuracy converged deadlocked exhausted   idle  rounds   turns  ms/episode
single-seat      1    0.640     1.000      0.000     0.000  0.000    2.00    2.00       0.011
math-panel       5    0.580     0.720      0.000     0.280  0.000    2.30    8.62       0.058
math-swarm      50    1.000     1.000      0.000     0.000  0.000    6.66   54.00       0.549
```

`ms/episode` is wall-clock time from one release build and varies between
machines. Every other column is deterministic for this seed set. The same
numbers, with a different wall clock, are pinned in
`crates/tinyhivemind-lang/examples/hive_view/README.md`.

## Findings

The expected ordering held only for the swarm.

1. **The swarm decided correctly on all 50 seeds**, against 64% for one
   seat. It converged every time, on a mean of 54 of its 150 turns over 6.66
   rounds.
2. **The five-seat panel was below one seat, at 58%.** The cause is
   termination: 28% of panel episodes exhausted their fifteen-turn budget with
   every advocate silenced by a grounded objection, and an undecided episode
   counts as incorrect. Two mechanisms produce this:
   - Wrong answers are correlated. Half of all wrong derivations land on the
     problem's single classic mistake, so wrong seats agree with each other
     rather than scattering.
   - A wrong checker silences correct advocates. The checker objects to up to
     two advocates of rival answers per turn. When it is wrong, its grounded
     objections remove the correct answer's support.
3. **The swarm's advantage is partly composition, not size alone.** Its ten
   `computer` seats are exact on a residue slice and never claim more than
   they know, so they can rule out the plus-or-minus-one mistakes. The panel
   has no such seat. This run can't separate the effect of fifty seats from
   the effect of those exact calculators. A swarm without computers, or a
   panel with one, would be the next arm to run.

## What this does not show

These are simulated seats under a stated error model. The run measures how the
protocol (blind rounds, grounded objection, quorum, and the turn budget)
behaves under that model. It says nothing about the quality of any real model.
The ranges were chosen once, not fitted, and a different error model could
reorder the panel and the single seat. The swarm's desk structure is not
exercised, because the offline loop runs every seat on one floor and routes no
referrals.

## What should change

- The panel's fifteen-turn budget with a quorum of three is too tight to
  recover from a wrong checker. A larger budget or a revealed round that lets
  a silenced advocate re-ground its claim is worth measuring before the
  panel is presented as a working configuration.
- Add the two ablation arms in finding 3 before claiming that size, rather
  than the residue calculators, carries the swarm's result.

## Update: routed desks and live runs

**Date:** 2026-10-11, later the same day. The sections above are the original
record and are kept as written; this section supersedes the math-swarm
numbers in them.

`hive_view` now runs a multi-desk package as one floor per desk: each desk has
its own journal and episode and takes its share of the package's quorum and
budget (for math-swarm, a quorum of 6 of 10 and 30 of the 150 turns). Desks
reach each other only through core's referral fold, along declared links, in
pipeline order (`conjecture`, `compute`, `prove`, `verify`, `synthesis`),
within `max_hops: 4`; synthesis's ending is the hive's. The error model,
ranges, problems and seeds are unchanged. Same command, release build:

```text
50 seeds, same problem per seed for every package; correct = exact answer

package      seats accuracy converged deadlocked exhausted   idle  rounds   turns  ms/episode
single-seat      1    0.640     1.000      0.000     0.000  0.000    2.00    2.00       0.009
math-panel       5    0.580     0.720      0.000     0.280  0.000    2.30    8.62       0.044
math-swarm      50    0.980     1.000      0.000     0.000  0.000   12.54   84.90       1.207
```

**How finding 1 changes.** The routed swarm was right on 49 of 50 seeds
(0.98), not all 50, and still converged on every seed. It spent a mean of 85
turns over 12.5 rounds instead of 54 over 6.66: every desk holds its own blind
round, and the five desks decide one after another, each on a quorum of six
of its own ten seats rather than 26 of fifty. The ordering is unchanged
(swarm well above one seat, panel below it), and findings 2 and 3 stand as
written. The caveat in "What this does not show", that the desk structure is
not exercised, no longer holds offline.

**Live runs.** `--live` makes every seat turn a real OpenRouter chat
completion, with core still deciding every round and ending and the desk
router still routing every referral. Requests carry no `tools`, `plugins` or
web-search options, and model ids containing `online`, `search`, `sonar` or
`web` are refused. Seed 1, `openai/gpt-oss-120b:nitro`, one run each:

| Package | Problem | Outcome | Calls | Tokens | Cost | Wall |
| --- | --- | --- | --- | --- | --- | --- |
| math-panel | lattice-5x8 | correct | 6 | 4,453 | $0.0020 | 1.6 s |
| math-swarm | lattice-5x8 | correct, via synthesis, 13 rounds | 58 | 46,104 | $0.0186 | 14.5 s |
| math-swarm | digitsum-2pow50 | correct | 55 | not recorded | $0.0231 | not recorded |

```sh
OPENROUTER_API_KEY=... cargo run --release -p tinyhivemind-lang --example hive_view -- \
  crates/tinyhivemind-lang/examples/hives/math-swarm --run --live --seed 1 \
  --problem lattice-5x8 --max-calls 300
```

Artifacts (every request and response, `calls.ndjson`, `frames.ndjson`,
`usage.json`) land in `hive-runs/hive-view/<package>-seed<N>-<ms>/` at the main
checkout's root, or under `$HIVE_RUNS_DIR`. Three single-seed episodes show
the routed pipeline working end to end with a real model; they are not an
accuracy measurement.
