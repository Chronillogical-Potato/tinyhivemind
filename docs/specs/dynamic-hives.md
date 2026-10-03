# Dynamic hives with supplied OpenHuman agents

Status: accepted, 2026-10-03. Implementation contract:
[dynamic-hives plan](../plans/2026-10-03-dynamic-hives.md).

## Intended behavior

The host creates one OpenHuman runtime and supplies already instantiated agents.
Each agent retains its model, system prompt, policy, MCP connections, skills,
memory, and one continuing conversation across all joined hives. TinyHivemind
adds permanent tools and schedules attributed incoming messages. It does not
reconstruct a persona or create a second OpenHuman runtime.

A hive and a desk are the same identity. Public coordinator and adapter APIs use
only `hive_id`; existing pure core `desk_id`/`chat` fields are translated at the
boundary. Runtime agent IDs are globally unique within the shared runtime.

## Ownership and coordination

`tinyhivemind-core` remains pure and preserves its existing folds.
`tinyhivemind-hives` owns coordination and provides a replaceable storage port,
memory storage, and optional SQLite storage enabled by default. This is an
explicit extension of host-owned storage: hosts can supply the port, while the
coordinator provides usable implementations. Only `tinyhivemind-openhuman` links
OpenHuman. No application-specific types enter the coordinator or core.

Each agent has an ordered durable inbox. Its turns are serialized across hives;
different agents can execute concurrently within core round bounds. Each hive
has at most one active episode, and subsequent hive messages queue episodes.
The coordinator uses the existing `CompletionDriver` and `Conductor`, including
child conversations, nudges, approvals, broadcast budgets, and turn walls.
Message fanout alone does not implement this contract.

Claim work in durable FIFO order with stable agent-ID tie breaking within a
conductor round. Busy shared agents retain their place rather than starving
behind new arrivals. Failed/cancelled turns release agent and episode execution
reservations while recording failure; recovery never leaves a stale running lock.

Tool operations that send elsewhere enqueue work and immediately return a
receipt. They never await another agent's response. Episode actions carry an
explicit episode ID and may only affect the active assignment of the caller.
Cross-hive messaging cannot complete or silently change that assignment.

## Permanent tools and dynamic management

One named attachment supplies one stable tool family per agent. Tools are
advertised in the system catalogue and provider schemas on every continuing
turn, including when discovery/deferred packing is enabled. Definitions and
execution schemas share one source. New memberships change destinations, not
tool counts. Attachments compose with the host's factory and preserve the policy
for unrelated tools; core episode/membership admission checks remain mandatory.
Registering the identical shared factory under an existing attachment key is
idempotent; a conflicting source fails. Permanent tool names are explicit in
the host turn's permanent-name set, whose default is empty. Continuing catalogue
refresh uses a generic opt-in TinyAgents prefix-refresh capability, delivered
as an upstream dependency change before OpenHuman's integration change.

The normal family supports hive/agent discovery, hive reads, hive sends, direct
agent sends, asks, broadcasts, posts, and completion. Sender identity comes from
the bound agent, never a model-supplied field. Tools validate visibility,
membership, destination, episode, and thread attribution. `hivemind_read`
accepts exactly one hive or peer destination; peer reads expose only the caller's
durable direct conversation, including returned replies. The optional `after`
cursor is exclusive. Returned replies do not schedule automatic return turns.

Public APIs always support creating hives, registering supplied agents, and
joining/leaving hives while scheduling runs. Empty hives can exist, but delivery
to one fails clearly. A membership change affects subsequent turns; an active
turn finishes against its captured membership/episode snapshot. Removing a
membership blocks subsequent delivery and keeps already recorded history.

Management tools are opt-in. The host supplies authorization and, for creating
agents, a factory accepting a template plus validated configuration. The
factory returns an already configured agent from the same runtime. Failure
does not leave a partially registered agent or membership.
Templates are host-defined. Tool configuration carries references and ordinary
settings; credentials remain in host configuration and are never tool arguments.

## Durability and recovery

Accepting a message and recording its delivery queues is one transaction.
Stable message IDs deduplicate retries. Storage persists definitions,
memberships, message order, delivery state, session IDs, conductor checkpoints,
and pending episode actions. OpenHuman handles and closures are never serialized.
On restart the host reattaches agent handles before their pending work runs.

Mark a turn running durably before invoking its runner. Successful closure
atomically persists the returned session ID, acknowledgements, and episode
transition. A crashed or cancelled running turn becomes interrupted on recovery;
do not replay its uncertain external tool effects automatically. Pending turns
that never started remain eligible. Shutdown stops claiming work and waits for
active turns; forced cancellation follows the interrupted-turn path.

## Acceptance

Deterministic tests cover one agent/one hive, many agents/one hive, many agents
across hives, and one agent in three hives; configuration isolation, visibility,
bounded concurrency, continuing history, dynamic creation, authorization, and
restart behavior. Request capture proves every registered Hivemind tool appears
once in prompt and schemas without discovery on successive turns. Storage
contract tests run against memory and SQLite. Standalone examples construct
agents separately and demonstrate dynamic provisioning through a host factory.

Old embed/raw agent-construction runners and registry generation are removed.
Progress, usage, approval, and conductor behavior remain available through the
supplied-agent adapter. Migration documentation maps OpenCompany's previous
construction integration to registration and explains shared runtime identity.
