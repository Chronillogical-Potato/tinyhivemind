# 25. The driver names no harness; one crate links it

- **Status:** Proposed
- **Date:** 2026-09-22
- **Relates to:** [ADR 0013](0013-a-vendored-crate-is-an-example-dependency.md), [ADR 0020](0020-openhuman-embed-is-a-git-dependency-patched-locally.md), [ADR 0022](0022-the-episode-mcp-server-is-the-one-socket.md)

## Context

`tinyhivemind-openhuman` began as "OpenHuman agent bindings" and grew the
completion driver inside it: `CompletionDriver`, the ledger of queued
handoffs, budgets and open asks, the brief a seat is shown, the scheduling
order. By the time the binding was made generic over `BoundAgent`, the
crate's OpenHuman content was two lines in three thousand -- the default
type parameter and the `impl BoundAgent for openhuman_embed::Agent` -- and
every host that wanted the driver linked all of OpenHuman to get it, and
inherited the Rust floor OpenHuman sets.

Meanwhile the code that actually runs a seat on OpenHuman -- the `SeatRunner`
seam, the `openhuman-embed` runner reaching the tools over MCP, the raw
`OpenHumanSessionHost` runner handed them natively, the definition registry
a raw seat must be registered in and the library-host context it must run
under -- lived in an example binary. It was found and fixed through ten live
runs, and a host taking the crates as they were could reproduce none of it.

So the crate named for the harness held the part that needs no harness, and
the part that needs the harness was not in a crate.

## Decision

Two crates, split by whether they name OpenHuman.

**`tinyhivemind-driver`** is the completion driver: `BoundHive`,
`AgentBinding<A: BoundAgent>`, `CompletionDriver`, the ledger, the brief,
the order. It depends on the algebra, the runtime ports, the routing
surfaces and the hive folds, and on no harness. It joins the pure list
`assert-pure.sh` guards. `BoundAgent` has no default implementation: a host
implements its one method for whatever it runs seats with.

**`tinyhivemind-openhuman`** is the OpenHuman adapter, and the one crate
under `crates/*` that links a harness: `SeatRunner`, `EmbedRunner` with
`EmbedSeat`, `RawRunner` with `RawSeat`, the belt, the gate, the memory that
keeps nothing, and -- behind an `offline` feature -- the scripted model both
runners are proven against. It takes `openhuman-embed`, `openhuman`,
`tinytools` and `tinytools-agent` on ADR 0020's terms: git dependencies
pinned by rev, patched onto `vendor/openhuman` here, and patched by a host
onto its own tree so one OpenHuman is linked. The tool crates are pinned at
the commit OpenHuman's own tree vendors, which is what makes the `Tool` a raw
session takes the `Tool` this repository implements.
`assert-openhuman-pin.sh` checks all three revs against the submodule.

The example keeps what is an example's: scenarios, the journal, the loop,
the bench and the printing.

## Consequences

- A host that runs seats on OpenHuman takes `tinyhivemind-driver` and
  `tinyhivemind-openhuman`, writes four `[patch]` lines against its vendored
  OpenHuman, and chooses a runner. A host on any other harness takes the
  driver alone and implements `BoundAgent` and `SeatRunner`.
- The adapter is held to the same file coverage as every crate, so both
  runners are proven offline in its own tests, in one process, because the
  runtime and the definition registry are process-wide.
- ADR 0020's reasoning now covers four crates rather than one; its decision
  is unchanged.
- `OpenHumanHive` is `BoundHive`. Nothing else in the driver's surface moved.
