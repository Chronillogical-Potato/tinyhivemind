# math-swarm

Fifty seats in five desks of ten solving the same problems as `math-panel`.
Conjecturers guess, computers are exact on one residue slice each, provers
derive in full, verifiers object to advocates of answers they cannot
reproduce, and synthesis leads support the best-grounded answer and record it.
The desks are chained by topology (`conjecture -> compute -> prove -> verify ->
synthesis`, with synthesis feeding back to conjecture) and joined by referrals
into the verify desk and out of it to synthesis. The constitution raises the
caps to 200 turns and width 10; blind rounds run ten seats at a time; quorum is
a grounded majority of the room (26), the threshold
`EpisodePolicy::for_room(50)` derives.

| Path | Purpose |
| --- | --- |
| `hive.json` | Five desks of ten, topology, referrals, rounds of ten, quorum of 26 |
| `constitution.json` | `max_turns` 200, `max_width` 10 |
| `memory.json` | No memory |
| `sim.json` | The offline problem bank and seat behaviour (same ranges as the panel) |
| `roles/`, `seats/` | Charters and fifty seat prompts |

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
