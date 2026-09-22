# The driver under the benchmark

**Date:** 2026-09-22
**Status:** Recorded
**Code:** `cargo run --release -p tinyhivemind-openhuman --example bench -- --episodes 2000`
**Decisions:** [ADR 0021](../adr/0021-an-assignment-is-appended-rather-than-overwritten.md),
[`../notes/completion-episode-review.md`](../notes/completion-episode-review.md)
**Supersedes the code of:** [2026-09-21](2026-09-21-what-the-host-loop-costs.md),
whose findings stand

The prototype loop that produced the 2026-09-21 numbers has been replaced by
the queues, budgets, open asks and delivery watermarks now carried in
`tinyhivemind-openhuman`'s `DriverState`, and by a host loop written the way a
host writes one: propose a round, run it, commit what it said in the order it
landed, report delivery, repeat until quiescent. This record asks whether the
real driver reproduces what the prototype found, and what the port itself
turned up. Nothing here needs a model or a credential.

## What was predicted, before the numbers existed

1. **`queue-nocap` equals `queue` to the decimal.** The per-assignment budget
   never fired in the prototype; it should not fire here.
2. **Width one is cheaper and the outcome does not move.** `queue-w1` should
   cost roughly six-tenths of `queue` in turns and routes, at the same
   quiescence.
3. **Nothing stalls.** The prototype's "never woken" case is now a rule -- a
   seat is owed a turn until it has run for what it holds *and* been shown
   everything -- so `stalled%` should be zero in every arm.
4. **The delivery guard never fires.** A host that delivers before a turn
   should never have a completion refused as undelivered.

Three held. The fourth was wrong on the first run, and the way it was wrong is
the finding.

## The room

Unchanged from 2026-09-21: five members, one turn is one assignment, three in
ten turns discover work for a specialist, a turn lands within 24 ticks and a
routing call within 2 more, the Choice shaped after a live Jev probe. Two
differences in the harness rather than the room:

- **There is no `gate` arm.** The driver *is* the queue path; "busy means
  unreachable" is not a policy it offers. The prototype's measurement of it --
  gating discards a tenth of its routing spend -- stands as recorded.
- **Concurrency is modelled by landing order, not tasks.** Every row a wave
  produces is tagged with when it would have landed and committed in that
  order. A broadcast landing before its recipient's completion meets a working
  seat and is queued; one landing after meets a settled seat and is assigned.
  Same ratio, deterministic, and one thread.

## Results

2000 episodes, same rooms per arm, 1.5 seconds of wall clock.

```text
arm          quiescent%      95% CI  stalled%  turns/ep  routes/ep  waves/ep  assign  queued  drained  disch  unplaced  peakQ
queue             100.0  99.8-100.0       0.0      12.2       3.61       6.6    3.71    3.49     3.49   0.00      0.00      3
queue-nocap       100.0  99.8-100.0       0.0      12.2       3.61       6.6    3.71    3.49     3.49   0.00      0.00      3
queue-w1          100.0  99.8-100.0       0.0       7.1       2.10       7.1    1.14    0.96     0.96   0.00      0.00      2
queue-w4          100.0  99.8-100.0       0.0      12.2       3.60       4.6    3.81    3.38     3.38   0.00      0.00      3
```

Against the prototype's `queue` row -- `12.5` turns, `3.76` routes, `7.3`
waves, `4.06` assigned, `3.46` queued -- the driver is within two percent on
every column that has a counterpart. The port did not change what the policy
costs.

**Prediction 1 held.** `queue-nocap` is `queue`, column for column. The budget
is insurance for a chain this room does not produce, and it costs nothing.

**Prediction 2 held.** `w1` spends `7.1` turns and `2.10` routes to `queue`'s
`12.2` and `3.61`; quiescence is identical. `queued` falls from `3.49` to
`0.96`: a single-recipient broadcast lands on a busy seat far less often,
because there is one recipient to be busy rather than two.

**Prediction 3 held.** Zero stalled in every arm. The seat that the prototype
could lose -- assigned, never asked -- is owed a turn by construction now.

**Prediction 4 was wrong, and it was wrong in the host loop.** The first run
refused `70%` of episodes with `participant a2 was assigned at 11 but delivered
only through 6`. The bench reported delivery to the seats in a round *after*
the round; a seat assigned by a broadcast mid-round was not in that round, so
its watermark stayed where its last turn left it, and its next completion was
refused. Delivering before each turn -- what a turn is shown is what it is
checked against -- fixed every one. The guard did exactly what it exists to
do, and the thing it caught was a host that had the order wrong.

**`w4` is no longer `w2` exactly.** Turns and routes are identical, but `w4`
runs `4.6` waves to `w2`'s `6.6`. In the driver `round_width` bounds the round
the host is offered as well as the broadcast fan-out, so a wider setting runs
wider, shallower waves for the same work. The prototype separated the two.
Width above two is still inert on the routing side -- `3.60` routes to `3.61`
-- because only one runner-up clears `20%` under a realistic Choice.

**Every queued handoff was drained.** `queued` equals `drained` in every arm:
a handoff held for a working seat was handed over at that seat's completion,
never lost. That equality is the invariant ADR 0021 was written for, and the
first run the driver has been measured under holds it exactly.

## What the port found in the code

Two things, both small, both real.

**An unplaceable broadcast left its author asleep.** A `Clarify` plan routes to
nobody, and the driver returned no actions and no re-wake; a host that
reported delivery would then find the author stalled with work it still held.
The author is now owed another turn. The unit test for it also found that a
clarify-shaped evaluation *escalates* on the primary pass and, with no
reasoning router, fails over to a fallback responder -- so a real `Clarify`
needs the reasoning pass to say so too. The test now does.

**A test router that is not a whole Choice is rejected, not accepted.** The
port's routers named one candidate and no contributions. `valid_domain`
requires the distribution to name every eligible candidate plus `none`, sum to
the scale, carry one contribution per candidate, and put the responder on top;
anything less is rejected to the fallback route. Three routers were passing
through that path by coincidence -- in a two-seat episode the fallback
responder *is* the other seat -- and one test was failing because of it. They
now build a whole Choice, and the tests exercise the accepted path they were
written for.

## What this does not measure

The same absences as before: no accuracy axis, and the termination bounds --
budget, discharge, the full queue returning work to its author -- never
engaged (`disch` and `unplaced` are `0.00` throughout). `ask` is not in the
room; the open-question rule is covered by the fuzz invariants and the ledger
tests, not by a cost here. The pathological chain the bounds exist for still
has no measurement of its own.
