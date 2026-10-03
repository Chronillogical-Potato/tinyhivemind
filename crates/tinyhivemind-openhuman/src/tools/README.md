# Native hive tools

`types.rs` declares the nine base tools and four optional management tools.
The same field declarations render JSON schemas and validate execution inputs.
Unknown arguments, including a supplied sender, are rejected. Integer sequences
must be nonnegative; recipient lists contain strings.

`mod.rs` binds attribution to the agent ID and accesses the coordinator through
weak references. Reads and sends are admitted by coordinator membership and
visibility checks; episode actions require the explicitly active episode.
Management additionally calls the host authorizer before mutation or factory.
`test.rs` checks definitions, argument validation, messaging, authorization and
membership changes.

`hivemind_read` requires exactly one of `hive_id` or `agent_id`. Hive reads may
include `thread`; direct reads return only the bound caller's conversation with
that peer, including durable runner replies. `after` is an exclusive sequence
cursor in both modes. Reading replies never schedules another turn, so agents
can inspect results and explicitly send follow-ups without automatic reply loops.
`direct_test.rs` exercises the native send/read path and its destination schema.
