# Run

The host side of one episode: a journal, a roster, and the step loop — the
whole contract a consuming host must honour. Fold, run the single turn the
library authorized, append it, commit the returned state, repeat; the library
never appends, never waits, and never calls back into the host. See
[`../README.md`](../README.md) for the benchmark this drives.

| file | what it does |
| --- | --- |
| [`mod.rs`](mod.rs) | Wiring: the `Participant` contract, the host-owned journal in `Host`, and the `run_episode*`/`drive` entry points that thread one episode from an opened state to a finished `EpisodeReport`. |
| [`turns.rs`](turns.rs) | Per-turn machinery: a turn's audience, appending it, and running one off-floor exchange round — called from the step loop once per authorized turn or exchange round. |
| [`scoring.rs`](scoring.rs) | The `EpisodeReport` shape and the running `Tally` that fills most of it: what an episode cost, what it decided, and whether the decision was any good. |
