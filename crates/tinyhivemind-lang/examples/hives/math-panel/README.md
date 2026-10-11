# math-panel

Five seats on one desk solving small exact problems: a conjecturer, two
solvers, a checker and a lead. Every seat is unreliable on its own; the panel
decides by a grounded quorum of three, and the checker objects to advocates of
an answer it cannot reproduce. Compare it with `math-swarm`, which solves the
same problems with fifty seats.

| Path | Purpose |
| --- | --- |
| `hive.json` | One desk, five seats, blind opening round of five, quorum of three |
| `constitution.json` | Default caps |
| `memory.json` | No memory |
| `sim.json` | The offline problem bank and seat behaviour |
| `roles/`, `seats/` | Charters and prompts |

## Simulation (`sim.json`)

`sim.json` is neither canonical nor Markdown, so `parse` drops it; only the
`hive_view` example reads it. It holds:

- `problems`: small problems whose answers `hive_view` computes exactly from
  `kind` and `args` (`lattice`, `modsum`, `digitsum`, `multiples`, `sqdiff`,
  `primes`). Correct means equal to that computation.
- `roles`: each role's behaviour and the range, in parts per million, that each
  seat's accuracy is drawn from with the run's seed.
- `errors`: how a wrong answer is chosen — the problem's classic mistake, or
  the answer plus or minus one.
- `slices`: the moduli a partial calculator knows the answer modulo.
- `baseline`: the single seat `--compare` runs alone.

The ranges are shared by every math package and were fixed before any run, so
a seat in the swarm is no better than its counterpart on the panel. See the
`hive_view` README for what each behaviour does.
