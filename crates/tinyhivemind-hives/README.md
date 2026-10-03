# tinyhivemind-hives

Durable coordination of host-supplied agents across dynamic hives. A hive and
a desk have one identity. Each registered agent has one continuing session and
one globally serialized turn stream, even when it belongs to several hives.

Use `Coordinator` with `MemoryStorage`, default-feature `SqliteStorage`, or a
host implementation of `Storage`. The host supplies `AgentRunner` handles from
one runtime; this crate never constructs agents or serializes live handles.
The OpenHuman-specific boundary lives in `tinyhivemind-openhuman`.

See [source modules](src/README.md), the
[accepted specification](../../docs/specs/dynamic-hives.md), and the
[repository overview](../../README.md).
