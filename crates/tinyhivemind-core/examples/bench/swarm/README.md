# Swarm

Several channels solving one problem, and the messages that cross between
them: one journal and one episode per desk, plus a scheduler that lets a
member spend its turn asking *another channel* a question instead of arguing
in its own. The asking, the routing, and the answer's way home are the
library's real `referral` fold; everything else here is the host side a
consumer would write. See
[`../README.md#several-channels`](../README.md#several-channels) for the
experiment this runs and its arms.

| file | what it does |
| --- | --- |
| [`mod.rs`](mod.rs) | Entry point: `drive_swarm`, the top-level loop that runs a federation of desks under one of the swarm arms. |
| [`board.rs`](board.rs) | `Board` — the scheduler for one federation-wide run: owns the `SwarmHost`, the referrals each desk has queued for another channel, and the running `SwarmReport`. |
| [`member.rs`](member.rs) | `SwarmSim` — wraps `sim::SimAgent` unchanged, adding the one thing a federation adds: knowing which channel it is on and asking another before backing anything in its own. Also the small bookkeeping for recording how a desk ended and what the federation settled on. |
| [`format.rs`](format.rs) | The wire text members exchange across a referral (`readings`/`restate`, read and write the same numeric-view line) and `line`, which renders one transcript entry for `--trace`. |
