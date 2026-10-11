# hive_view

Look at a hive package, and watch it run without a model. `hive_view` reads a
package directory, hands the strings to `tinyhivemind_lang::parse`, lowers it,
and then renders it or drives an episode through core's `hive::step`. The
example does the file IO; the library stays pure, so
`.github/scripts/assert-pure.sh` is unchanged.

```sh
H=crates/tinyhivemind-lang/examples/hives
cargo run -p tinyhivemind-lang --example hive_view -- $H/review-board             # text tree
cargo run -p tinyhivemind-lang --example hive_view -- $H/incident-response --mermaid
cargo run -p tinyhivemind-lang --example hive_view -- $H/math-swarm --json
cargo run -p tinyhivemind-lang --example hive_view -- $H/review-board --run [--trace out.jsonl]
cargo run -p tinyhivemind-lang --example hive_view -- $H/math-panel --run --frames --seed 3
cargo run -p tinyhivemind-lang --example hive_view -- $H/math-swarm --compare --seeds 50 --table
cargo test -p tinyhivemind-lang --example hive_view
```

`--table` prints the comparison as a table (with wall-clock milliseconds per
episode) instead of the NDJSON stream; `--run --compare` is the same as
`--compare`.

## How a run works

- **One room.** A one-desk package runs on that desk. A package with several
  desks runs one episode on the *hive floor*: every seat of every desk, in
  declared desk order, so quorum is hive-wide. Topology and referrals are drawn
  by `--mermaid` and reported by `--json`, but this offline loop does not route
  referrals between desks.
- **Core decides.** Each step is `hive::step` over the transcript; each
  authorized seat speaks against `project_for` (so a blind or concurrent seat
  sees only what core allows); its line is appended; repeat. The ending is the
  `HiveStep` core returns. Standings in frames come from `trace::read` and
  `quorum::standings`, the same folds `step` uses.
- **Who speaks what.** `script.json` (seat id to lines) is replayed when
  present; past the end of its lines a seat writes `!question`. Without one, a
  seeded default script proposes one of `option-a`/`b`/`c` blind, then
  supports the leader (60%), objects to a rival's advocate (25%) or adds
  evidence (15%). A package with `sim.json` runs the math simulation. In the
  commit phase every speaker records the topic that carried in its own view.
- **`--trace`** writes core telemetry (`Stamped` `TraceEvent`s) as JSONL with
  a one-tick-per-event clock, the format `examples/lab/viewer` reads.
  `TraceEvent` carries no message content, which is why frames exist.

## The math simulation

Every seat draws an accuracy from its role's `sim.json` range (parts per
million) with the seed, then derives a private answer for the problem: right
with that probability, otherwise the problem's classic mistake (50%) or the
answer plus or minus one (25% each). Answers are computed in `math.rs`.

| Behaviour | Blind round | Once the room is revealed |
| --- | --- | --- |
| `guess` | Propose its guess | Support the leading topic once |
| `solve` | Propose its derivation | Object to one advocate of a rival answer per turn |
| `check` | Propose its derivation | Object to up to two advocates of rival answers per turn |
| `slice` | Propose the exact answer, or post its residue | Object to up to two advocates its residue rules out; support the leading admitted answer once |
| `lead` | Propose its derivation | Support the leading topic whenever it changes |

Objections are `!object >N ^M`, grounded in the seat's own proposal; support
is `!support #T ^N`, citing a proposal. The ranges are shared by every math
package and were fixed before the first run: a swarm seat is no better than
its panel counterpart. The single-seat baseline is one `solve` seat on a
one-member desk with a quorum of one.

### Result, 50 seeds

`--compare --seeds 50 --table`, release build:

```text
package      seats accuracy converged deadlocked exhausted   idle  rounds   turns  ms/episode
single-seat      1    0.640     1.000      0.000     0.000  0.000    2.00    2.00       0.013
math-panel       5    0.580     0.720      0.000     0.280  0.000    2.30    8.62       0.061
math-swarm      50    1.000     1.000      0.000     0.000  0.000    6.66   54.00       0.613
```

The swarm decided correctly on every seed within at most 150 turns. The
five-seat panel did *worse* than one seat: its errors are correlated (half of
all wrong derivations land on the same classic mistake), a wrong checker
silences correct advocates, and 28% of its episodes exhausted their fifteen
turns with every advocate silenced. The intended ordering (swarm > panel >
single) holds only for the swarm. The swarm's edge comes from size *and* from
roles the panel lacks: ten exact residue checks rule out the plus-or-minus-one
mistakes. These are simulated seats; this measures the protocol under a stated
error model, not model quality.

## Wire format (`--json`, `--run --frames`, `--compare`)

This is the contract the viewer UI is built against. Fields may be added;
none may be renamed or removed. Additions so far: `seats[].memory`,
`episode.quorum_window`, `episode.require_grounded`, `missions`,
`sim.scripted`, `turns[].sequence`, `standings[].supporters`,
`results[].topic`, and `rows[].deadlocked`/`exhausted`/`idle`. `correct` in an
`end` frame is `false` (not `null`) for a math run that did not decide.
`standings[].object` counts advocates silenced by a grounded objection.

Binary: `cargo run -p tinyhivemind-lang --example hive_view -- <package-dir> [mode] [knobs]`
Built binary path used by the viewer: `<repo>/target/debug/examples/hive_view`.

Modes:

- (none) — text tree of the lowered package
- `--mermaid` — mermaid flowchart
- `--json` — ONE JSON object (structure), see below
- `--run` — human-readable episode lines + ending
- `--run --frames` — NDJSON frames on stdout (one JSON object per line)
- `--run --trace <out.jsonl>` — also write core telemetry Stamped TraceEvents as JSONL
- `--compare --seeds N [--packages a,b,...]` — NDJSON compare stream (math packages; always includes baseline "single-seat")

Knobs: `--seed N` (u64, default 1), `--round-width N` (1..=64), `--turn-budget N`
(1..=2000), `--problem <id>` (math only). `--round-width` sets both the blind
and the revealed width. Errors: non-zero exit, message on stderr.

### `--json` structure

```text
{
  "id": "math-swarm", "name": "Math swarm", "summary": "<first paragraph of package README.md, or empty>",
  "roles":  [{"id":"verifier","charter":"<first line of charter body>"}],
  "seats":  [{"id":"verify-03","label":"verify-03","role":"verifier","desks":["verify"]}],
  "desks":  [{"id":"verify","name":"Verify","description":"...","members":["verify-01",...],"responder_mode":"auto"}],
  "topology":  [{"from":"conjecture","to":"compute"}],
  "referrals": [{"from":"verify","to":"synthesis"}],
  "goals":  [{"id":"...","description":"...","subgoals":[]}],
  "episode": {"turn_budget":120,"round_width":10,"revealed_width":10,"blind_round":true,"quorum_threshold":6},
  "sim": {"kind":"math","problems":[{"id":"lattice-6x6","statement":"..."}]}   // or {"kind":"script"}
}
```

### `--run --frames` NDJSON

```text
{"type":"start","package":"math-swarm","seed":3,"seats":50,"problem":{"id":"lattice-6x6","statement":"...","answer":"924"}}   // problem null for non-math
{"type":"round","round":1,"phase":"<HiveTurn phase, snake_case>","visibility":"blind"|"full",
 "turns":[{"seat":"verify-03","desk":"verify","reason":"<snake_case reason>","saw":4,"of":9,"content":"!object 920 ...",
           "answer":"920","correct":false}],          // answer/correct null when not math or no answer in line
 "standings":[{"topic":"924","support":5,"object":1,"committed":false}]}
...
{"type":"end","ending":"converged"|"deadlocked"|"exhausted"|"idle","topic":"924"|null,"correct":true|null,"rounds":7,"turns":58}
```

### `--compare` NDJSON

```text
{"type":"seed","seed":1,"problem":"lattice-6x6","results":[{"package":"single-seat","seats":1,"ending":"converged","correct":false,"rounds":1,"turns":1},{"package":"math-panel",...},{"package":"math-swarm",...}]}
...
{"type":"summary","seeds":50,"rows":[{"package":"math-swarm","seats":50,"accuracy":0.86,"converged":0.94,"mean_rounds":6.2,"mean_turns":51.0}]}
```

## Files

| File | Purpose |
| --- | --- |
| `main.rs` | Argument parsing and mode dispatch |
| `load.rs` | Read a package directory into the `parse` map; README summary; `sim.json`/`script.json` |
| `render.rs` | Text tree and mermaid flowchart |
| `structure.rs` | The `--json` structure |
| `episode.rs` | The room (one desk or the hive floor) and the `hive::step` loop |
| `script.rs` | `script.json` replay and the seeded default script |
| `math.rs` | The problem bank and its exact answers and classic mistakes |
| `sim.rs` | Seat behaviours for math packages |
| `rng.rs` | Seeded `SplitMix64` |
| `run.rs` | Knobs, room preparation, the single-seat baseline, `--run` output and the JSONL trace sink |
| `frames.rs` | The NDJSON wire types |
| `compare.rs` | `--compare` |
| `test.rs` | Offline tests, including the pinned wire form |
