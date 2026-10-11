# hive_view

Look at a hive package, and watch it run, offline or against a real model.
`hive_view` reads a package directory, hands the strings to
`tinyhivemind_lang::parse`, lowers it, and then renders it or drives it
through core's `hive::step`. The example does the file IO (and, with
`--live`, shells out to `curl`); the library stays pure, so
`.github/scripts/assert-pure.sh` is unchanged.

```sh
H=crates/tinyhivemind-lang/examples/hives
cargo run -p tinyhivemind-lang --example hive_view -- $H/review-board             # text tree
cargo run -p tinyhivemind-lang --example hive_view -- $H/incident-response --mermaid
cargo run -p tinyhivemind-lang --example hive_view -- $H/math-swarm --json
cargo run -p tinyhivemind-lang --example hive_view -- $H/review-board --run [--trace out.jsonl]
cargo run -p tinyhivemind-lang --example hive_view -- $H/math-panel --run --frames --seed 3
cargo run -p tinyhivemind-lang --example hive_view -- $H/math-swarm --compare --seeds 50 --table
OPENROUTER_API_KEY=... cargo run -p tinyhivemind-lang --example hive_view -- $H/math-swarm --run --frames --live --max-calls 300
cargo test -p tinyhivemind-lang --example hive_view
```

`--table` prints the comparison as a table (with wall-clock milliseconds per
episode) instead of the NDJSON stream; `--run --compare` is the same as
`--compare`.

## How a run works

- **One floor per desk.** A one-desk package runs on that desk. A package of
  several desks runs one episode per desk: its own journal, its own
  `EpisodeState`, and `hive::step` over that desk's members only. Each desk
  takes its share of the package's quorum threshold and turn budget (for
  math-swarm, 6 of 10 and 30 of 150), widths capped at its size.
- **Core decides.** Each step is `hive::step` over a desk's transcript; each
  authorized seat speaks against `project_for` (so a blind or concurrent seat
  sees only what core allows); its line is appended; repeat. The ending is the
  `HiveStep` core returns. Standings in frames come from `trace::read` and
  `quorum::standings`, counting only the desk's members, as `step` does.
- **Who speaks what.** `script.json` (seat id to lines) is replayed when
  present; past the end of its lines a seat writes `!question`. Without one, a
  seeded default script proposes one of `option-a`/`b`/`c` blind, then
  supports the leader (60%), objects to a rival's advocate (25%) or adds
  evidence (15%). A package with `sim.json` runs the math simulation. In the
  commit phase every speaker records the topic that carried in its own view.
  With `--live` a model writes every line (below).
- **`--trace`** writes core telemetry (`Stamped` `TraceEvent`s) as JSONL with
  a one-tick-per-event clock, the format `examples/lab/viewer` reads.
  `TraceEvent` carries no message content, which is why frames exist.

## Desk routing

`desks.rs` is the bench federation's host (`tinyhivemind-core/examples/bench/
swarm/`) on a package's own desks: a pass gives every unfinished desk one
move — answer a referral that arrived, or take its next round. Messages cross
desks through core's `referral` fold only: the mention is read by core's
mention grammar, and the routing, the hop budget and the answer's return edge
are `referral(policy, input, roster, desks)`. The host adds what
`ReferralPolicy` leaves to a host:

- **Links.** A referral travels only along a declared, directed `topology` or
  `referrals` link; a return travels back along its forward's link. Anything
  else is refused `unlinked`. A desk that has ended refuses `desk_closed`.
- **Pipeline.** A link from an earlier-declared desk to a later one is forward.
  A desk opens once every desk with a forward link into it has ended, so
  math-swarm runs `conjecture`, `compute`, `prove`, `verify`, `synthesis` in
  turn. The last declared desk's ending is the hive's.
- **Reports.** A converged desk's committing seat refers its answer down each
  outgoing link: `!propose #<answer> ^1 <Desk> settled on ...` and `@#<desk>`.
  The row lands in the receiving journal authored by that seat, so core never
  counts it there; members must back it in their own turns. A report carries
  the deepest hop that reached its desk, so `max_hops` bounds the pipeline:
  at math-swarm's 4, synthesis's feedback reports are refused
  `hop_limit_reached`. A desk that did not converge reports nothing.
- **Asks.** A seat line with an `@#desk` (or an off-desk agent) mention is a
  forward referral starting at hop 0. The seat core chooses answers on its own
  desk (a round frame with phase `answer`), and core's `Return` carries the
  answer home. A desk may ask at most twice (`FORWARD_CAP`): a constant width
  bound, as `ReferralPolicy`'s documentation asks of a host. The simulation
  never asks; live models may.
- **Policy.** `policies.referral` is honoured as core reads it: `enabled:
  false` refuses every report `disabled`, `reach: local` refuses desk
  mentions, `returns: false` keeps answers on the far desk.

A simulated seat reads a report as one more advocate in its own view (it
can support or object to it); core's quorum never counts it.

## Live mode (`--live`)

`--live` makes every seat turn a real chat completion on OpenRouter, playing
the seat: its prompt (the persona line and job), its role charter, its desk
and linked desks, the trace grammar, and **its own projection** of its desk's
journal — referral rows included, blind rounds still blind. Core still decides
every round and ending; `desks.rs` still routes every referral.

- **Key.** `OPENROUTER_API_KEY` from the environment, handed to `curl` as a
  `--config -` header on stdin, so it is in no argv, file or log.
- **Model.** `--model <id>`, else `OPENROUTER_MODEL`, else
  `openai/gpt-oss-120b:nitro` (what `live_self_edit.rs` uses).
- **Replies.** Reduced deterministically to the first line core's trace reader
  accepts with its required fields (bullets, emphasis, code fences and topic
  punctuation such as `#1,287.` repaired), plus at most one `@#desk` line. A
  reply with no valid trace line is recorded as `note: ...`, which never votes.
- **Concurrency.** A round's calls run on scoped threads, at most
  `round_width` at once; replies land in the order core authorized.
- **Budget.** `--max-calls N` (default 300, at most 2000) is a hard cap: a
  round the remaining calls cannot fill ends the run `exhausted`.
  `--max-tokens N` (default 1200) per call. A `usage` frame reports calls,
  tokens and the cost OpenRouter returned. A 401, 402 or 403 ends the run with
  the redacted error; any other failed call is recorded as a `note:` line.
- **Artifacts.** `target/hive-live/<package>-seed<N>-<ms>/`: every request
  body and response, `calls.ndjson` (seat, desk, raw reply, recorded line),
  `frames.ndjson` and `usage.json`. Never the key.

### No web access

Enforced three times. Every gallery seat denies `web_search`, `web_fetch`,
`browser` and `http` and allows none (`tests/gallery_packages.rs`). The only
request body sent carries no `tools`, `tool_choice`, `functions`, `plugins` or
`web_search_options` (pinned in `test/live.rs`, and checked again before each
send). And a model id with `online`, `search`, `sonar` or `web` in it (such
as `:online`) is refused before any call.

Personas are cosmetic offline: the simulation draws behaviour from
`sim.json`, so the offline numbers do not depend on them.

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

`--compare --seeds 50 --table`, release build, one floor per desk:

```text
package      seats accuracy converged deadlocked exhausted   idle  rounds   turns  ms/episode
single-seat      1    0.640     1.000      0.000     0.000  0.000    2.00    2.00       0.010
math-panel       5    0.580     0.720      0.000     0.280  0.000    2.30    8.62       0.051
math-swarm      50    0.980     1.000      0.000     0.000  0.000   12.54   84.90       1.395
```

The swarm converged on every seed and was right on 49 of 50, in a mean of 85
turns over its five desks. Run on one combined floor before desks were routed
it was right on all 50 in 54 turns: the pipeline spends more turns (every desk
holds its own blind round) and its five desks decide one after another, each
on a quorum of six. The five-seat panel did *worse* than one seat: its errors
are correlated (half of all wrong derivations land on the same classic
mistake), a wrong checker silences correct advocates, and 28% of its episodes
exhausted their fifteen turns with every advocate silenced. These are
simulated seats; this measures the protocol under a stated error model, not
model quality.

## Wire format (`--json`, `--run --frames`, `--compare`)

This is the contract the viewer UI is built against. Fields may be added;
none may be renamed or removed. Additions so far: `seats[].memory`,
`episode.quorum_window`, `episode.require_grounded`, `missions`,
`sim.scripted`, `turns[].sequence`, `standings[].supporters`,
`results[].topic`, `rows[].deadlocked`/`exhausted`/`idle`, `round.desk`,
`round.desk_round`, `desks[].episode`, phase `answer`, the `referral`, `desk_end` and `usage`
frames, `end.desk`, and `start.live`/`start.model`. `correct` in an
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
and the revealed width; on several desks `--turn-budget` is the hive's total,
shared out. Live: `--live` (with `--run`), `--model <id>`, `--max-calls N`
(1..=2000), `--max-tokens N` (64..=8192). Errors: non-zero exit, message on
stderr.

### `--json` structure

```text
{
  "id": "math-swarm", "name": "Math swarm", "summary": "<first paragraph of package README.md, or empty>",
  "roles":  [{"id":"verifier","charter":"<first line of charter body>"}],
  "seats":  [{"id":"verify-03","label":"verify-03","role":"verifier","desks":["verify"]}],
  "desks":  [{"id":"verify","name":"Verify","description":"...","members":["verify-01",...],"responder_mode":"auto",
              "episode":{"turn_budget":30,"round_width":10,"revealed_width":10,"quorum_threshold":6}}],   // this desk's floor
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
     // --live adds "live":true,"model":"<id>"
{"type":"round","round":1,"desk":"verify","desk_round":1,"phase":"deliberate"|"commit"|"answer","visibility":"blind"|"full",
 "turns":[{"seat":"verify-03","desk":"verify","reason":"<snake_case reason, or referral>","saw":4,"of":9,"content":"!object 920 ...",
           "answer":"920","correct":false}],          // answer/correct null when not math or no answer in line
 "standings":[{"topic":"924","support":5,"object":1,"committed":false}]}   // the desk's members only
{"type":"referral","round":10,"kind":"report"|"forward"|"return","from_desk":"verify","to_desk":"synthesis",
 "seat":"verify-01","target":"synthesis-01"|null,"content":"!propose #924 ^1 ...\n@#synthesis","hops":4,
 "accepted":true|false,"reason":"hop_limit_reached"|"unlinked"|"desk_closed"|"width_cap"|"disabled"|...,  // reason only when refused
 "sequence":2|null}                                   // where it landed in to_desk's journal
{"type":"desk_end","desk":"verify","ending":"converged"|...|"unopened","topic":"924"|null,"correct":true|null,"rounds":2,"turns":11}
...
{"type":"usage","calls":58,"prompt_tokens":40070,"completion_tokens":6034,"total_tokens":46104,"cost_usd":0.01855,"wall_ms":14525,"exhausted_calls":false}   // --live only
{"type":"end","ending":"converged"|"deadlocked"|"exhausted"|"idle","topic":"924"|null,"correct":true|null,"rounds":7,"turns":58,"desk":"synthesis"}
```

`round` counts rounds across every desk. A `referral` frame's `round` is that
counter when it was routed; `hops` is the child hop it travelled at, or the
hop it was refused at. `desk_end` frames (one per desk, several desks only)
come before `end`, whose `desk` (several desks only) names the desk whose
ending it is. Core's refusal reasons are `NoReferralReason` in `snake_case`;
`unlinked`, `desk_closed` and `width_cap` are this host's.

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
| `episode.rs` | One desk's room, the `Speaker` trait, and the `hive::step` loop |
| `hive.rs` | A package of several desks: one room per desk, each desk's share of the policy, the declared links |
| `desks.rs` | The pass scheduler that runs one floor per desk and routes referrals between them |
| `script.rs` | `script.json` replay and the seeded default script |
| `math.rs` | The problem bank and its exact answers and classic mistakes |
| `sim.rs` | Seat behaviours for math packages |
| `rng.rs` | Seeded `SplitMix64` |
| `run.rs` | Knobs, plans (one room or a hive), the single-seat baseline |
| `output.rs` | `--run` output (human or frames), the JSONL trace sink, live usage |
| `frames.rs` | The NDJSON wire types |
| `compare.rs` | `--compare` |
| `live/` | `--live`: the OpenRouter transport, prompts and reply parsing, the live speaker |
| `test/` | Offline tests, including the pinned wire form |
