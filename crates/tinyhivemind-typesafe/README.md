# `tinyhivemind-typesafe`

The reusable TypeSafe System One boundary for TinyHiveMind routing.

It constructs the batched Jev Choice and Noul questions, converts exact wire
responses to fixed-point routing evaluations, and classifies retryable provider
statuses. `SystemOneTransport` is its only waiting port. The crate contains no
HTTP client, runtime, API key handling, or application types.

TinyHiveMind code selects the Choice maximum as primary and concurrently routes
to other eligible Choice options strictly above 20%, subject to the configured
round width. Nouls remain independent audit signals rather than recipient
selectors.

See [`src/README.md`](src/README.md).

The standalone [`OpenHuman example`](../../examples/openhuman/README.md) runs
the router through a minimal embedded agent without adding OpenHuman to this
crate's dependency graph or Rust-version contract.
