# Feature modules

One directory per feature area, each answering one question from a fold over
arguments the caller supplies. `lib.rs` exposes the modules and carries the
crate-level overview; this file points to each module's own README. The public
API stays under module names such as `desk` and `mention`.

| module | question it answers |
| --- | --- |
| [`aside`](aside/README.md) | how a private, off-transcript note is scoped to the readers it names |
| [`approval`](approval/README.md) | whether one typed side-effecting action is allowed, denied, or must be put to one person |
| [`chat`](chat/README.md) | which stored chat id names which conversation, and which four spellings mean the default desk |
| [`desk`](desk/README.md) | what a desk is, and who is on it once the declared blueprint is merged with runtime overlays |
| [`dispatch`](dispatch/README.md) | does one committed reply start a child turn, and for whom |
| [`error`](error/README.md) | the crate-wide `Error` enum and `Result` alias every fallible function returns |
| [`masking`](masking/README.md) | which spans of a message body are fenced or inline code, so no grammar parses inside them |
| [`mention`](mention/README.md) | who does an authored `@` name address, resolved against the live roster and desks |
| [`referral`](referral/README.md) | does one turn cross to another desk, and how its one answer carries back |
| [`responder`](responder/README.md) | which single agent answers one message, walking the deterministic ladder |
| [`roster`](roster/README.md) | who is here — active, retired, or tombstoned agents and the people signed in with them |
| [`runtime`](runtime/README.md) | attributed session projection, sharing, and ports for host-owned data |
| [`hive`](hive/README.md) | bounded task division and group deliberation |
| [`embed`](embed/README.md) | conversation surfaces and semantic routing |
| [`typesafe`](typesafe/README.md) | exact System One wire types and Jev routing |
| [`driver`](driver/README.md) | host-neutral completion scheduling and committed-event folds |
| [`telemetry`](telemetry/README.md) | stamped run events and the sink a host receives them on |
