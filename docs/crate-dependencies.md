# How the crates use each other

This page records direct, normal Cargo dependencies between the eight workspace
crates. An arrow points from the crate that depends on another crate to the
crate it imports. It says nothing about which crate calls first at runtime.
The individual crate READMEs explain the APIs used at each edge.

```mermaid
flowchart TD
    runtime["tinyhivemind"] --> core["tinyhivemind-core"]
    hive["tinyhivemind-hive"] --> runtime
    hive --> core
    embed["tinyhivemind-embed"] --> runtime
    typesafe["tinyhivemind-typesafe"] --> embed
    typesafe --> runtime
    driver["tinyhivemind-driver"] --> hive
    driver --> embed
    driver --> runtime
    driver --> core
    tools["tinyhivemind-tools"] --> runtime
    openhuman["tinyhivemind-openhuman"] --> driver
    openhuman --> tools
    openhuman --> runtime
```

| Crate | Direct workspace dependencies | What it takes from them |
| --- | --- | --- |
| [core](../crates/tinyhivemind-core/README.md) | None | Owns the pure desk, roster, mention, approval, and responder algebra. |
| [runtime](../crates/tinyhivemind/README.md) | core | Uses pure decisions and re-exports core's public surface. |
| [hive](../crates/tinyhivemind-hive/README.md) | runtime, core | Reads attributed session types; uses core masking and errors; re-exports runtime. |
| [embed](../crates/tinyhivemind-embed/README.md) | runtime | Reuses sequence and fixed-point probability types for host-neutral routing. |
| [TypeSafe](../crates/tinyhivemind-typesafe/README.md) | runtime, embed | Implements embed's `Router` with the runtime's probability scale. |
| [driver](../crates/tinyhivemind-driver/README.md) | runtime, core, embed, hive | Combines utterances, desk validation, routing, and completion folds. |
| [tools](../crates/tinyhivemind-tools/README.md) | runtime | Renders and interprets the runtime's speech vocabulary. |
| [OpenHuman](../crates/tinyhivemind-openhuman/README.md) | runtime, driver, tools | Runs driver decisions on OpenHuman seats with native tools. |

Two public re-exports shorten a host's dependency list. `tinyhivemind`
re-exports core, and `tinyhivemind-hive` re-exports the runtime. A re-export does
not reverse the Cargo dependency. An application can depend on a narrower
crate when it does not need the layer above it.

For a completion episode, the hive crate defines the pure episode state and
folds. The driver combines those folds with routing and committed-event
bookkeeping. The OpenHuman adapter runs seats selected by the driver. The
tools crate records seat calls for a host to expose through its tool interface.
The host still owns the transcript, durable
appends, and agent lifecycle.
