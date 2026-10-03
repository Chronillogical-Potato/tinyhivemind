# `runner`

The seam a host loop steps a seat through. `SeatRunner::open` registers the
turn and the read window, `turn` runs it, `close` drains what was called;
open and close are provided, because both runners record into the same
`EpisodeTools`. `Lane` is where a turn runs, desk or thread, for the host's
own bookkeeping; `TurnJob` is a running turn, and `TurnResult` what became
of one -- it replied, it failed, or it parked on the host. `RunnerKind` names a runner
and states its one sentence on the mechanics for the standing contract.

`test.rs` proves both runners through the seam against the scripted model,
in one process, because the runtime and the definition registry are
process-wide.
