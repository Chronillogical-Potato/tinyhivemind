# Instantiated agents

This module binds canonical route ids to opaque agent handles created by an
embedding host. TinyHiveMind neither constructs nor interprets the handle, so
an OpenHuman `Agent` keeps ownership of its provider configuration, transcript,
session compaction, tools, and runtime state.

| File | Purpose |
| --- | --- |
| `mod.rs` | Module documentation and public exports. |
| `types.rs` | Registry, resolved route views, and validation failures. |
| `test.rs` | Identity, ordering, missing-agent, and clarification behavior. |
