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
