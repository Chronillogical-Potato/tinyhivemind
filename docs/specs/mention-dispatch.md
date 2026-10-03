# Mention dispatch

> **Current API note.** [ADR 0027](../adr/0027-retire-unused-adapters-and-optional-mechanisms.md) retired the runtime waiting wrapper and some standalone query entry points described below. The retained pure folds and transcript views are documented in the current crate READMEs.


**Status:** Implemented
**Owner:** tinyhivemind maintainers

## Problem

An agent reply can explicitly address another agent, but resolving the mention
does not safely start the next turn. Hosts need a single, idempotent dispatch
edge that cannot fan out, loop without a bound, or create a second journal.

## Goals

- Decide from committed reply data whether one direct agent mention may start
  one child turn.
- Bound chains with a finite, host-supplied `max_hops` and no library hard cap.
- Put durable, authorization-aware, exactly-once enqueueing behind one host
  port.
- Keep the feature explicitly disabled until a host opts in.

## Non-goals

- Fan-out for person, desk, or `@everyone` mentions.
- Library-owned persistence, retries, environment configuration, or model
  selection.
- OpenCompany integration or a live provider test; those follow this library
  phase.

## Proposed behavior

`MentionDispatchPolicy { enabled, max_hops }` is supplied for every decision.
Zero hops disables dispatch. There is no compiled-in ceiling: values through
`u32::MAX` are valid, while child-hop arithmetic is checked.

`mention_dispatch` evaluates in this order: disabled policy, exhausted hop
budget, inactive source, then the first reading-order nonquiet `Agent` mention.
Quiet, person, desk, and everyone mentions are skipped. If that first direct
agent mention is the source itself or names an inactive target, the decision
fails closed and does not try a later mention. Otherwise the decision contains
one canonical `MentionTurnRequest` whose child hop is the parent hop plus one.

Dispatch keys bind the committed trigger sequence. The request also binds the
conversation (`desk_id` and optional thread root), source id, target id,
content, and child hop. The runtime `MentionTurnQueue::enqueue_once` port
receives that single owned request and returns `Enqueued`, `Already`, or
`Refused { reason }`. `EnqueueRefusal` contains only the expected
`Unauthorized`, `TargetUnavailable`, and `FeatureDisabled` reasons, so a
successful enqueue cannot be represented as a refusal.
`dispatch_mention` invokes it zero or one time, returns refusals as outcomes,
and returns unexpected host failure as a typed error with its source. It never
retries or considers another mention.

All public payload fields are required in JSON. Enum tags and variants use
snake case. A host maps its richer runtime conversation to the pure dispatch
conversation; this is a snapshot, not a host type or callback. The runtime
adapter canonicalizes every General alias to `GENERAL_DESK`, while named desk
ids remain exact and case-sensitive. Thread roots remain exact, including the
difference between a desk channel and each individual thread.

## Trust assumptions

The hop bound is only as good as the counter it reads, and the spec has never
said so. `input.hop` arrives inside the request a host assembles,
`mention_dispatch` compares it against `policy.max_hops`, and the `child_hop`
it hands back travels out through the host's storage and transport before
anything reads it again. The library never authenticates either number.

**What the library guarantees.** Given an honest `hop` and an honest roster:
a chain cannot exceed `max_hops`, because each decision admits at most one
child and increments by exactly one under checked arithmetic; the hop budget is
tested before any mention is read; and no self mention, inactive target, or
inactive source is ever dispatched to.

**What it does not.** That the `hop` it was handed is the one the parent
decision produced. A participant able to author an enqueue request can also
write `hop: 0` into it, and the loop the bound exists to stop then runs
unbounded with every individual decision comfortably inside its budget. The
same holds for `max_hops` and `enabled`, which are supplied per call and are
therefore only as trustworthy as whatever computed them.

**What a host must do.** Where every participant is trusted — one process
assembling requests from rows it wrote itself, which is the expected
deployment — nothing. Where they are not, the counter must be carried in
something a participant cannot edit: derived from the host's own stored trigger
row rather than from the message, or, across a process boundary, carried in an
assertion the receiving side verifies. OpenBot carries its depth cap in a
signed assertion for exactly this reason
([`../research/grok-bots/copilotkit-openbot.md`](../research/grok-bots/copilotkit-openbot.md)).

This library will not sign it. It has no transport to sign for and no key
material to sign with, and a signature it could neither issue nor verify at the
boundary that matters would be decoration. The assumption is stated here so a
host does not read the bound as stronger than it is.

## Refusals, and what is offered

Two rules the OpenBot survey produced, and where each stands here.

**A run at the cap is not offered the action.** Refusing at the edge teaches a
model that the action exists and then wastes a turn on it; withholding it is
cheaper and clearer. The decision fold has this shape: `max_hops` is tested
before any mention is read, so a reply at the cap constructs no
`MentionTurnRequest` at all and there is nothing to refuse.

The text the model reads now has it too, and this is no longer a gap.
`TeamBriefing::system_text` withholds the dispatch sentence entirely, and
`system_text_with_dispatch(MentionDispatchContext { policy, hop })` states it
only when `may_dispatch()` holds — the policy is enabled and the run is inside
its hop budget, which are exactly the two guards this fold tests before it
reads a mention. A run that could not dispatch is not told that it can. See
[`sessions.md`](sessions.md) for the briefing itself and
[`responders.md`](responders.md) for the same discipline on the selection path,
where a selector is asked only when its answer can be acted on.

**A decline should be a sentence, not only a type.** The library owns the
words. Every reason it returns — the seven `NoDispatchReason` variants and the
three `EnqueueRefusal` variants — renders through `Display` as one lowercase
sentence an acting model may repeat to a person, and a host neither invents nor
translates it.

The wording is bounded by
[ADR 0009](../adr/0009-a-refusal-renders-what-the-caller-already-holds.md),
which settles this against
[ADR 0008](../adr/0008-an-approval-decision-is-total.md) rather than leaving the
two rules pulling: a refusal that is distinguishable is a refusal an agent can
probe, and `TargetInactive` versus `NoDirectAgentMention` is exactly the pair
that turns a failed dispatch into a roster oracle. Audience separation — the
reason for the operator's log, the sentence for the agent — was necessary and
not sufficient, because it constrains the channel and not the information.

The rule is that a refusal renders its own sentence only when what it discloses
is something the caller already holds: the policy it supplied, the hop it
supplied, or its own identity. A refusal that turns on whether a *named other*
exists, is active or is reachable renders the one shared sentence exported as
`dispatch::NO_AVAILABLE_TARGET`. So `Disabled`, `HopLimitReached`,
`SourceInactive` and `SelfMention` are worded apart, while
`NoDirectAgentMention` and `TargetInactive` are worded identically.

Reachability is itself a channel, which decides the two remaining cases.
`HopOverflow` is defensive rather than reachable today — `mention_dispatch`
rejects `input.hop >= max_hops` before the `checked_add`, and `max_hops: u32`
bounds every admitted hop to at most `u32::MAX - 1`, so the increment cannot
actually overflow — but its wording is fixed for the day a wider counter makes
it live: it renders `HopLimitReached`'s sentence verbatim. Every
`EnqueueRefusal` is reached only
past that same point, so `Unauthorized` and `TargetUnavailable` render the
shared sentence and `FeatureDisabled` renders `Disabled`'s verbatim. Success is
exempt and can be: a dispatch that runs proves the target exists anyway.

`Display` carries the agent's sentence and `Debug` the operator's variant,
because the rendering that is safe to hand to a model has to be the one `{}`
reaches for. See [`responders.md`](responders.md) for how the same rule lands on
the selection path, where nothing is withheld at all.

## Invariants and constraints

- One committed message creates at most one turn.
- The host owns storage, child-turn records, and the only idempotency record.
- `enqueue_once` must atomically re-read and validate the committed reply,
  current policy, authorization, target availability, and conversation binding,
  then durably enqueue at most once under the conversation-and-trigger key.
- **The idempotency check is decided in the same statement as the insert.**
  Reading the key and then writing if it was absent is a check that holds only
  while nothing else is enqueueing, and the case it has to hold in is precisely
  the opposite one. Decide it in the write — a conditional insert, an upsert, a
  unique index the write races against — and read `Already` off that write's
  own outcome. A committed mention can start at most one child turn here, so
  the count OpenBot had to move inside its insert is a count this library never
  keeps; the race it lost to survives as the idempotency check, which has the
  same shape and the same failure.
- **The lease clock is the store's.** Whatever lease or visibility timeout the
  host puts on a claimed child turn must be computed *and* compared inside the
  store, never as a moment the calling process computes and the store then
  compares against its own. Two clocks pretending to be one is how a turn gets
  delivered twice, which spends a model call this contract has already promised
  is spent once.
- **The attempt count is visible.** A queue that folds attempts into a status
  hides the only distinction a retry decision can safely rest on: an attempt
  count of one means the turn has certainly not run, and anything higher means
  it may already have spent money and written a message. The port answers
  `Already` for a duplicate rather than a number, so the count lives on the
  host's own row and has to be readable there rather than inferred.
- The library never reads environment variables or enables the feature through
  a Cargo feature. OpenCompany's eventual adapter default is two hops, but its
  first integration remains disabled until deliberately enabled.
- Host refusal is final for the call; there is no library retry or fallback.

## Acceptance criteria

- Pure tests pin wire forms and cover disabled/zero, hop limits 1 and 2, a
  large limit, `u32::MAX`, inactive/self targets, reading order, quiet and
  non-agent mentions, and exactly-one decisions.
- Every refusal variant is classified against ADR 0009 by a wildcard-free
  `match`, so a variant added later does not compile until it is classified,
  and a test then asserts the withheld ones are indistinguishable and the rest
  are not.
- Runtime tests prove zero-or-one queue calls, refusal mapping, source-preserved
  host failure, canonical bound-scope keys, and concurrent/retried duplicate
  enqueue producing exactly one durable key and one durable child turn.
- The queue contract documents the required atomic host transaction, the
  same-statement idempotency check, the store-owned lease clock, and the
  visible attempt count.
- Host integration tests revalidate stored rows and transaction rollback.
  A later live test proves two agents exchange at least one attributed turn
  through a real provider; neither is simulated in this crate.

## Open questions

- Host integration and live verification. Nothing else is open for the library
  phase. Which reasons may be rendered distinctly is no longer open: every
  variant is classified in
  [ADR 0009](../adr/0009-a-refusal-renders-what-the-caller-already-holds.md) and
  each classification is pinned by a test.
