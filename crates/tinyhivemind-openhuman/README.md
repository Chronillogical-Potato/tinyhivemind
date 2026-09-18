# `tinyhivemind-openhuman`

This crate binds canonical TinyHiveMind agent identities to already-created
OpenHuman agents. It validates one desk graph, constructs host-neutral routing
requests, resolves desk-private messages, and advances completion episodes only
after the host supplies a committed sequence.

This is a first-class workspace library, not an example-only integration. It
depends directly on `openhuman-embed`, which raises the root workspace's Rust
floor to 1.96. It does not make the algebraic crates impure:
`tinyhivemind-core` and `tinyhivemind-hive` retain their dependency-purity
checks, while this adapter remains the explicit OpenHuman-specific boundary.

Canonical member, routing-candidate, and binding sets must match exactly;
blank ids and the router-reserved `none` id are refused. Canonical ids are
independent of `OpenHuman` runtime ids, so one cloned agent handle can be bound
under different canonical ids in separate hives.

The completion driver's serializable caller-owned state contains the episode
and global freshness receipts. Reconstructing a driver and resuming that state
therefore recognizes committed posts, DMs, broadcasts, and completion calls
without routing, delivery, or scheduling them twice. Concurrent commit
batches are folded by actual host sequence, independent of vector order.
Pending work, broadcasts, and private desk routes are all bounded by the
driver's `round_width`; a caller's broader semantic routing policy is clamped
to that bound. Broadcast routing sees only the current episode participants
other than the author. The driver selects each author's fallback from those
participants in deterministic scheduling order.

The host still owns OpenHuman runtimes and sessions, the transcript, durable
append operations, and scheduling. See [`src/README.md`](src/README.md) for the
source layout.
