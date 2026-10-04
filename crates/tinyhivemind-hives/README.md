# tinyhivemind-hives

Durable coordination of host-supplied agents across dynamic hives. A hive and
a desk have one identity. Each registered agent has one continuing session and
one globally serialized turn stream, even when it belongs to several hives.

Use `Coordinator` with `MemoryStorage`, default-feature `SqliteStorage`, or a
host implementation of `Storage`. The host supplies `AgentRunner` handles from
one runtime; this crate never constructs agents or serializes live handles.
The OpenHuman-specific boundary lives in `tinyhivemind-openhuman`.

Register an existing conversation atomically with
`register_agent_in_session(registration, session_id)` before a live scheduler
claims it. Hosts can create empty hives and join/leave registered agents while
the scheduler runs; unstarted removed seats are retired before claiming.
An active turn retains its captured membership.

Direct sends return durable receipts without awaiting peers. `read_direct`
exposes only the caller/peer pair, including returned replies, and starts no
turn. Hive reads enforce membership and private thread audiences.

Storage snapshots contain no live handles. On restart reattach runners with
durable agent IDs from the newly constructed runtime. Pending work resumes;
previously running work becomes interrupted without automatic replay.
Failed finalization carrying a valid completed session retains that binding
while suppressing acknowledgements and staged output.

See [source modules](src/README.md), the
[accepted specification](../../docs/specs/dynamic-hives.md), and the
[repository overview](../../README.md).

Start with the [offline coordinator examples](examples/README.md) to see a
reviewer handoff and one continuing agent session across three hives.
