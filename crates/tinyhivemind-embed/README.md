# `tinyhivemind-embed`

Host-neutral integration types for applications embedding TinyHiveMind.

The crate names conversation surfaces explicitly and composes a semantic
router with deterministic eligibility, escalation, and fallback rules. It owns
no storage, transport, credentials, or OpenCompany/OpenHuman types. A host can
place already-instantiated agent handles in `AgentRegistry<A>` and resolve an
accepted routing plan without TinyHiveMind constructing or recreating agents.
For a completion episode, `tinyhivemind-driver` binds those handles through
`BoundHive<A>`; its graph also checks that desk members and route candidates
match. `AgentRegistry<A>` remains available to hosts that use routing without
the completion driver.

See [`src/README.md`](src/README.md) and
[`docs/specs/jev-first-routing.md`](../../docs/specs/jev-first-routing.md).

## How it relates to the other crates

This crate depends directly on
[`tinyhivemind`](../tinyhivemind/README.md) for the shared `Sequence`
and fixed-point `Probability` types. It does not create a second version
of those values. It adds `ConversationRef`, routing candidates, the
`Router` port, and the rules that accept or reject a router's evaluation.

[`tinyhivemind-typesafe`](../tinyhivemind-typesafe/README.md) implements
that `Router` port with Jev questions.
[`tinyhivemind-driver`](../tinyhivemind-driver/README.md) uses routing
plans and candidates when it assigns or hands off work. The embed crate knows
neither implementation's transport nor its agent handles.

See the [workspace dependency map](../../docs/crate-dependencies.md).
