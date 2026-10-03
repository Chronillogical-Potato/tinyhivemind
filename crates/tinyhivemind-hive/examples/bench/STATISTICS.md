# Reading the benchmark's second table

> Historical results from benchmark arms retired in October 2026. The current
> benchmark no longer runs these arms. The `ladder+dir` arm received the true
> topic from `room.truth`, so its routing scores include an oracle leak.

Split out of [`README.md`](README.md), which is capped at 500 lines. That
file says what the arms are and what they scored; this one says how to read
the columns under them and what the intervals do and do not license.

A second table is printed under the first, in `metrics/mod.rs`:

```text
arm       correct %          95% CI  fact %  to-fact  knows %  defers/ep  route %  cost/ep    rho
ladder         57.6       56.2–59.0       —        —        —          —        —     1.00      —
vote           78.5       77.4–79.6       —        —        —          —        —    15.00      —
hive           73.3       72.0–74.5       —        —      0.0        0.0        —     6.16   0.01
hive+          82.1       81.0–83.1       —        —      0.0        0.0        —     6.75   0.19
```

`95% CI` is a Wilson score interval on `correct %`, chosen over the plain
normal approximation because several arms here land near 0% or 100%, where the
plain interval can cross outside `[0, 100]` and its coverage is worst. Under
each row, a paired-bootstrap comparison line reports the same arm against
`vote` at equal turns, e.g. `hive+ − vote: +3.6 [+2.1, +5.0]`: the accuracy
difference and its 95% interval, resampling episode indices together for both
arms because they decided the *same* rooms.

`fact %` and `to-fact` report how often, and how late, the room's decisive
member — a `--specialists` topic expert or the `--hidden-profile` fact-holder —
put its knowledge on the floor **before the commit boundary**, over the
episodes that *had* such a member. It counts a topiced `!evidence` deposit and
nothing else: an earlier version of this column counted the member's opening
`!propose`, read 100% on every arm of every run, and measured only that
everybody gets a turn in the blind round. A deposit landing at or after the
commit boundary is compute the room paid for and could not use, and is scored
as a miss.

`knows %` is the share of episodes in which a `BidReason::Knows` bid won the
floor at least once — the directory's holder of the contested topic, brought
out because the transcript says it knows something it has not said. Only an arm
that folds a directory can reach it. `defers/ep` is turns spent on `!defer`.
`route %` is the share of the scoreable episodes in which the responder ladder
picked the decisive member as its one responder. `rho` is the
circularity number `docs/specs/expert-delegation.md` obliges this benchmark to
print: at the end of every episode the harness folds `directory()` over that
episode's own journal — always at `DirectoryPolicy::DEFAULT`, whatever the arm
asked for, so the number is comparable across arms — and takes Pearson's
correlation on tie-averaged ranks, in exact integer arithmetic, between each
member's total directory weight and the number of turns it took, averaged over
the sample. A column reads `—` rather than `0.0` wherever the arm structurally
cannot produce that number (a control arm never deliberates, so it never has
an expert or a rho; a uniform room names no expert, so nothing can be routed
right or wrong).

**What `rho` means.** Near `1.0` the directory reproduces the speaking order
and has learned nothing except who talked — the failure
`docs/research/delegation.md` names *who spoke becomes who is thought to
know*. At or below zero it is reading grounded deposits and other members'
citations rather than turn count. It is printed to two decimals rather than
one: on a hidden profile under `--blind-evidence`, `hive+defer` reads `-0.02`
and `hive+dir+defer` reads `-0.00` — one fewer digit would collapse both to
`0.0` and erase the boundary between weight that has been zeroed by deferral
and weight that merely tracks turns weakly.

Under the ordinary opening the default bench does not cluster: `hive` reads
`0.01` and `hive+ref` reads `0.49`, with the directory arms in between at
`0.19`. Under `--blind-evidence` the same bench moves higher and tighter,
`0.61`–`0.72`. On a hidden profile it is lower again under the ordinary
opening, `0.08`–`0.33`, and under `--blind-evidence` it separates by
mechanism: `0.32` for `hive+`, `0.37` for `hive+dir`, and at or below zero
once `!defer` is folded in — `-0.02` for `hive+defer`, falling to `-0.08` at
`--defer-cap 2`. Depositing before arguing lifts weight off pure turn count;
deferring is what pushes it to zero or below.

`--json` prints one flat JSON object per arm, one per line, ahead of both
tables, covering every column of both (as `fact_pct`, `to_fact`, `knows_pct`
and `defers_per_episode`) plus `expert_led` — the share of
episodes in which the decisive member authored the first `!propose` for the
topic the room went on to decide, which the tables have no room for. `--stats-check` runs a small set of
known cases through `wilson`, `paired_bootstrap` and `spearman_milli` — the
statistics live in an example, so `cargo test` never exercises them — and
exits `0` or `1`.

