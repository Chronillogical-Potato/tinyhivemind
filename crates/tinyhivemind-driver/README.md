# `tinyhivemind-driver`

The completion driver. It binds canonical TinyHiveMind agent identities to
the handles a host runs its seats with -- any `BoundAgent`, since the driver
stores a bound handle and hands it back with a pending round, and never runs
one -- validates one desk graph, constructs host-neutral routing requests,
resolves desk-private messages, and advances completion episodes only after
the host supplies a committed sequence.

It names no harness. Its dependencies are the algebra, the runtime ports, the
routing surfaces and the hive folds, and it is in the pure list
`.github/scripts/assert-pure.sh` guards. The OpenHuman adapter is
[`tinyhivemind-openhuman`](../tinyhivemind-openhuman/README.md): it
implements `BoundAgent` for OpenHuman's two kinds of seat and runs their
turns, and a host on some other harness writes that one method itself.

Canonical member, routing-candidate, and binding sets must match exactly;
blank ids and the router-reserved `none` id are refused. Canonical ids are
independent of a handle's runtime id, so one cloned handle can be bound under
different canonical ids in separate hives.

The driver's serializable caller-owned state contains the episode and global
freshness receipts, and -- beside the episode fold -- a `Ledger` of what the
fold cannot record: handoffs queued for a busy recipient, broadcasts charged
to each assignment, and questions whose answers are still owed.
Reconstructing a driver and resuming that state recognizes committed posts,
DMs, broadcasts, and completion calls without routing, delivery, or
scheduling them twice. Concurrent commit batches are folded by actual host
sequence, independent of vector order. Pending work, broadcasts, and private
desk routes are all bounded by the driver's `round_width`; a caller's broader
semantic routing policy is clamped to that bound. Broadcast routing sees only
the current episode participants other than the author. The driver selects
each author's fallback from those participants in deterministic scheduling
order.

Above the driver sits the `Conductor`: one episode as a host steps it, the
desk and a child episode for every conversation an `ask` opens, with the
rules between them -- conversations run first and conclude to the asker, a
stalled seat or a silent askee is told once, what a wave said lands in the
channel it belongs to, a refused completion is explained, walls end what
will not. It appends nothing: it hands the host notes to append, commits to
append and report the sequence of, and events to log.

The host still owns the runtime and sessions, the transcript, durable append
operations, and scheduling. See [`src/README.md`](src/README.md) for the
source layout, and `examples/bench/` for the driver priced with no model.
