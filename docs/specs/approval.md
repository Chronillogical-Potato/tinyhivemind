# Approval: a pure gate for a side-effecting action

- **Status:** Implemented
- **Owner:** `crates/tinyhivemind-core`, with one port in `crates/tinyhivemind`
- **Reading:** [`../research/grok-bots/README.md`](../research/grok-bots/README.md)
- **Decisions:** [ADR 0008](../adr/0008-an-approval-decision-is-total.md),
  [ADR 0009](../adr/0009-a-refusal-renders-what-the-caller-already-holds.md)

## Problem

This library can say who is here, who a mention addresses, who takes the next
turn, and how a room reaches a decision. It cannot say whether the thing that
decision leads to is allowed to happen. There is no approval concept anywhere
in the workspace: not a policy, not a grant, not a verdict type.

The Grok Bot survey found that gap to be the largest one it could evidence.
Six of twelve projects gate side-effecting actions, and in every one of them
the *decision* is already a pure function separated from the IO that enacts it:

- gawkbot's `action.Classify(ClassifyInput{ReadOnly, State, HasGrant})` is a
  switch with no IO returning one of six verdicts, and `actionGrantActive`
  is a fail-closed liveness predicate over `(grant, now)`.
- devspace's `selectAcpPermissionOption(options, writeMode, provider)` is a
  pure choice over a supplied option list; replying to the RPC is a thin
  wrapper around it.
- The reconstructed 0.18 desktop app's `localToolApprovalCovers` is four lines
  deciding whether a stored approval covers a new request, and is reused by
  two processes for that reason.
- OpenBot's computer gateway resolves the target against its own snapshot,
  asks the policy, writes the audit row, and only then acts — in that order,
  because a caller-supplied label otherwise evades "never click Submit".
- The orange book's trigger taxonomy — payment, sensitive action, low
  confidence — is a decidable predicate over an action descriptor.

That is the core/port line this workspace already draws, arrived at
independently five times. The survey also found what happens when the line is
not held: gawkbot's own gate is one long function that classifies, posts a
card, polls, and writes audit inline, and the project ships an ungated local
tool loop beside it that runs `bash` with no check at all.

## Goals

- Decide, from arguments the caller already holds, whether one side-effecting
  action may proceed, must be refused, or has to be put to a person.
- Express a standing grant as a pure liveness and coverage predicate, with no
  clock and no storage of its own.
- Make consent non-retroactive: a grant issued later must not cover a request
  minted earlier.
- Fail closed on every path, including the paths that are ordinarily errors.
- Add no dependency to `crates/tinyhivemind-core`, and no host type anywhere.

## Non-goals

- **Executing anything.** No tool port, no command surface, no process spawn.
  This crate never enacts, and nothing in `ApprovalDecision` schedules work.
- **A policy language.** No CEL, no expression evaluator, no rule DSL. See
  [Why there is no policy language](#why-there-is-no-policy-language).
- **Audit storage.** The host owns the record of what was asked and answered,
  as it owns every other log here. gawkbot's `ApprovalAuditEntry` is the right
  shape and the wrong owner.
- **Credential handling.** No tokens, no secrets, no OAuth. An approver's
  identity arrives as a `Person` already projected by the host.
- **Sandboxing.** The resource predicate defined below is a *lexical* path
  containment test, not a kernel boundary. devspace's `isPathInsideRoot` is
  honest about this and so is this spec: it is a logical fence, and a symlink
  defeats it, because a pure crate cannot call `realpath`.
- **A second dispatch edge.** An `Ask` is not a mention and never becomes a
  `MentionTurnRequest`.

## Proposed behavior

A new `approval` module in `crates/tinyhivemind-core`, sibling to `dispatch`
and `referral`.

### The entry point

```rust
pub fn approve(
    request: &ApprovalRequest,
    policy: &ApprovalPolicy,
    grants: &[StandingGrant],
    refusals: &[RememberedRefusal],
    roster: &Roster<'_>,
    desks: &DeskSet<'_>,
    now: Millis,
) -> ApprovalDecision
```

`refusals` is the caller-held snapshot of this epoch's remembered refusals,
read the same way `grants` is — required to enforce step 6 below.

It returns `ApprovalDecision`, not `Result<ApprovalDecision>`. That is a
deliberate departure from this crate's rule that fallible public functions
return `Result<T>`, and it is the subject of
[ADR 0008](../adr/0008-an-approval-decision-is-total.md): a gate that can fail
is a gate that can be bypassed by failing.

`now` is `Millis(u64)`, a host-supplied monotonic reading. There is no clock in
this crate. This is `schedulerJobDue(job, now)`, which the survey records as
gawkbot's canonical due predicate for the same reason.

### The request

```rust
pub struct ApprovalRequest {
    pub epoch: ConsentEpoch,
    pub sequence: u64,
    pub call_id: String,
    pub actor_id: String,
    pub conversation: DispatchConversation,
    pub action: Action,
}

pub struct Action {
    pub verb: String,
    pub target: ActionTarget,
    pub effect: Effect,
}

pub enum ActionTarget {
    Named { name: String },
    Resource { path: String },
}

pub enum Effect { ReadOnly, Mutating, Unclassified }
```

`Effect` is declared by the host, not inferred here. gawkbot infers it from
about forty-five verb tokens, and its own note records the consequence: a
vendor whose action ids use an unanticipated verb slips through classified as
read-only. This crate will not guess. `Effect::Unclassified` is a valid input
and it denies.

`conversation` reuses `DispatchConversation` unchanged, so an action is bound
to the same canonical `(desk_id, thread_root)` pair a dispatch decision binds.

**Trust assumption: `actor_id` is host-authenticated**, the same boundary
[`mention-dispatch.md`](mention-dispatch.md#trust-assumptions) draws around
`input.hop`. `approve` never verifies it, so ADR 0009's claim that
`UnknownActor` discloses nothing new holds only if the host derived it from an
authenticated session rather than a caller-chosen value; otherwise the variant
becomes a roster oracle no code here can catch.

### The scope key

```rust
pub struct ScopeKey {
    pub actor_id: String,
    pub call_id: String,
    pub verb: String,
    pub effect: Effect,
    pub target: ActionTarget,
}
```

This is the reconstruction's `askKey` — one agent, one tool call, one action,
one target and one effect — kept as typed fields rather than one joined string.
`ScopeKey::render()` exists only for a host that wants one opaque dedupe token;
it is collision-free by construction, not convention — each field is
length-prefixed so an embedded NUL cannot fake a boundary, and `ActionTarget`'s
variant is an explicit leading tag so `Named` and `Resource` sharing a string
never render alike. The byte layout is pinned by a serde test.

A grant declares how far past its own call it reaches:

| `GrantScope` | Covers |
| --- | --- |
| `Call` | only requests carrying the same `call_id` |
| `Action` | any `call_id`, same `(actor_id, verb, effect, target)` |
| `Resource { root }` | any `call_id`, same `(actor_id, verb, effect)`, with both keys carrying resource paths lexically inside `root` |

`Call` is the reconstruction's default and `completeScope` retiring everything
not marked `outlivesScope`; `Action` is that mark; `Resource` is its
`resourcePath` pinning, which is how one grant survives a run of calls touching
the same file or the same terminal folder.

Path containment is devspace's `isPathInsideRoot` written as a pure fold over
already-normalized components: equal to the root, or below it, with any `..`
component anywhere in either path denying outright rather than being resolved.
The crate does not expand `~`, does not resolve symlinks and does not touch a
filesystem, and the rustdoc says so at the predicate itself.

### Standing grants and epochs

```rust
pub struct StandingGrant {
    pub scope: GrantScope,
    pub key: ScopeKey,
    pub granted_at_epoch: ConsentEpoch,
    pub granted_at_sequence: u64,
    pub granted_at: Millis,
    pub expires_at: Option<Millis>,
    pub revoked: bool,
}
```

`grant_live(grant, now, policy)` is a pure predicate, and it is false when the
grant is revoked, when `now >= expires_at`, when `now < granted_at` (a caller
handed a reading from before the grant existed, so the state is inconsistent
and the answer is no), when `expires_at` is `None` and the policy caps grant
lifetime, and when the declared lifetime exceeds `policy.max_grant_ttl`.

That last case **ignores the grant rather than clamping it**. gawkbot clamps to
a thirty-day `maxGrantTTL`. Clamping rewrites the record of what a human
agreed to and then acts on the rewrite; a grant that falls outside current
policy is evidence that policy changed or that the record is wrong, and in
both cases the correct answer is to ask again.

`ConsentEpoch(u64)` is a host-owned monotone counter that advances whenever a
person gives new direction. It carries the correctness property this spec
exists to state:

> A grant covers a request only if `(grant.granted_at_epoch,
> grant.granted_at_sequence) <= (request.epoch, request.sequence)`.

A grant minted at 12:01 does not authorize a call minted at 12:00 that is
still in flight. This is the reconstruction's `alwaysGrantedAtEpoch`, recorded
per agent precisely "so a call minted before the grant does not silently
benefit from it". Without the sequence half, two events inside one epoch order
arbitrarily and the same hole reopens at a finer grain.

A **remembered refusal** is the mirror image and is *not* symmetric:

```rust
pub struct RememberedRefusal {
    pub key: ScopeKey,
    pub scope: GrantScope,
    pub epoch: ConsentEpoch,
}
```

A refusal applies only when `refusal.epoch == request.epoch`. When the epoch
advances, stale refusals stop applying, which is the reconstruction's behavior
and the right one: a grant is a person widening authority and should outlive a
change of subject, while a refusal is a person answering *this* question under
*this* direction, and new direction retires it. The asymmetry is deliberate and
is pinned by a test.

### Policy

```rust
pub struct ApprovalPolicy {
    pub enabled: bool,
    pub default: DefaultVerdict,      // Deny | Ask — there is no Allow
    pub rules: Vec<ApprovalRule>,
    pub approver: ApproverRule,
    pub allow_grants: bool,
    pub max_grant_ttl: Option<Millis>,
}

pub enum DefaultVerdict { Deny, Ask }

pub struct ApprovalRule {
    pub effect: Option<Effect>,
    pub verb: Option<String>,
    pub target: Option<TargetPattern>,
    pub verdict: RuleVerdict,        // Deny | Ask | Allow
}

pub enum ApproverRule {
    Absent,
    Person { id: String },
    PerDesk { default: String, overrides: Vec<DeskApprover> },
}
```

`DefaultVerdict` has no `Allow` variant. Fail-closed is a property of the type
here, not of a code path that has to be got right, and `--unsafe` has no
spelling in this API. `enabled: false` denies everything; it is a kill switch,
not a bypass. A host that does not want an action gated does not call
`approve` for it. gawkbot's `WUPHF_UNSAFE=1`, its `--unsafe` flag and its
dry-run skip are each a hole in "every send waits for your click", and none of
them is reproduced.

`ApproverRule::Absent` deterministically denies with `NoApprover`.
`ApproverRule::PerDesk` resolves the request's `desk_id` through
`DeskSet::resolve_id` — the existing algebra, including its existing
`UnknownDesk` and `AmbiguousDesk` failures — and then resolves the chosen id
through `Roster::person`. **An approver is always a person.** `Ask` cannot name
an agent, because an agent approving another agent's side effect is not a gate,
it is a second agent. This is the one place where the shape suggested for this
work is narrowed rather than followed.

The two approver-resolution `DenyReason`s are distinct, not synonyms:
`UnresolvableApprover` is `DeskSet::resolve_id` itself failing (`UnknownDesk`
or `AmbiguousDesk`); `NoApprover` is desk resolution succeeding (or a bare
`Person` rule needing none) but the named id not naming an active
`roster.person`. A `Person` rule can therefore deny with `NoApprover` but
never `UnresolvableApprover`, having no desk lookup to fail.

### The decision

```rust
pub enum ApprovalDecision {
    Allow { basis: AllowBasis },
    Deny { reason: DenyReason },
    Ask { who: String, scope: GrantScope, key: ScopeKey, epoch: ConsentEpoch },
}
```

`Ask` carries the exact scope a grant minted from answering it may claim, so
the host cannot widen a question into a broader standing authority. `Allow`
carries `AllowBasis::Policy` or `AllowBasis::Grant { key }` so a host's audit
row records *why* rather than only *that*. When several live grants cover a
request, `key` is deterministic: the narrowest scope wins (`Call` before
`Action` before `Resource`), and a tie within one scope breaks on the lowest
`(granted_at_epoch, granted_at_sequence)` — so the basis is a function of the
grant *set*, matching order-independence below, not of `grants`' order.

`DenyReason` is enumerated and closed: `Disabled`, `MalformedRequest`,
`UnknownActor`, `UnclassifiedAction`, `PolicyDenied`, `RememberedRefusal`,
`UnresolvableApprover`, `NoApprover`, `NoRule`.

### Evaluation order

Deny beats allow at every level. This is OpenBot's gateway rule — deny beats
allow, absent policy denies, a broken rule denies — and it is deliberately
*not* the first-match-wins ladder `direct_responder` uses, because a ladder
lets a permissive rule placed first hide a restrictive one placed later.

1. `!policy.enabled` → `Deny { Disabled }`.
2. Empty `actor_id`, `call_id`, `verb`, or target string, or a target path with
   a `..` component → `Deny { MalformedRequest }`.
3. `roster.active_member(actor_id)` is `None` → `Deny { UnknownActor }`.
4. `action.effect == Unclassified` → `Deny { UnclassifiedAction }`.
5. Any matching rule with `RuleVerdict::Deny` → `Deny { PolicyDenied }`.
6. A refusal in this epoch whose scope covers the request →
   `Deny { RememberedRefusal }`.
7. `policy.allow_grants` and a live, covering, epoch-eligible grant exists →
   `Allow { Grant }`.
8. Any matching rule with `RuleVerdict::Allow` → `Allow { Policy }`.
9. Any matching rule with `RuleVerdict::Ask`, or `default == Ask` → resolve
   the approver; on success `Ask`, on failure `Deny { UnresolvableApprover }`
   or `Deny { NoApprover }`.
10. Otherwise `Deny { NoRule }`.

### Where the waiting goes

`approve` decides. Nothing in this crate waits for a person, because waiting is
IO and this crate does none.

The waiting belongs behind **one new port in `crates/tinyhivemind`**, sibling
to `MentionTurnQueue` and `ReferralQueue`, not in the host directly:

```rust
pub trait ApprovalGate: Send + Sync {
    async fn ask_once(&self, prompt: ApprovalPrompt) -> Result<ApprovalOutcome>;
}
```

It goes in the runtime crate rather than being left to each host because the
contract it needs is the one two existing ports already state and hosts already
implement: atomically re-read the committed request and current policy,
authorize, and durably record at most once under a key. The key is
`ScopeKey::render()` plus the request sequence, computed purely here — gawkbot
factors out `actionApprovalDedupeKey` with a comment saying it is "pure for
testability", and this is the same key with an owner. `ApprovalOutcome` is
`Asked`, `Already`, or `Answered { decision }`. `Already` means pending only;
a completed record must return its answer. Polling, timeouts, cards, and the
audit row stay with the host.

That port is the second half of this phase and is specified here only in
outline; the pure algebra lands first and is useful without it, because a host
that already has an approval UI needs only the decision.

### One message, one turn

This does not relax the invariant, and it cannot:

- `ApprovalDecision` has no variant that carries a turn, a mention, or a
  `MentionTurnRequest`.
- `Ask` names exactly one person. It is a question put to one addressee, not a
  broadcast to a desk, for the same reason `@everyone` is a list.
- `approve` never consults `direct_responder`, `mention_dispatch` or
  `referral`, and none of them consult it. A host that gates a dispatched turn
  calls `approve` before it calls `dispatch_mention`; the ordering is the
  host's and the library expresses no edge between them.

### Why there is no policy language

CEL, or any embedded expression evaluator, is rejected on four grounds.

1. It is a dependency, and `crates/tinyhivemind-core` takes none that it can
   avoid. An interpreter on the hot path of every gated action is exactly what
   `.github/scripts/assert-pure.sh` exists to keep out.
2. It converts every rule into a string that can fail to parse at evaluation
   time. Under "a broken rule denies", a policy language makes the failure mode
   the common one, and under any other reading it makes it an allow.
3. This crate prefers small typed APIs to stringly-typed ones. `ApprovalRule`
   is three optional matchers and a verdict; anything a host needs beyond that
   it can evaluate itself and hand in as a narrowed `ApprovalPolicy` snapshot.
4. Policy authoring is host configuration. A host with a rules engine keeps it,
   compiles it down, and passes the result — which is the same snapshot-not-
   callback boundary every other type here crosses.

### Rendering a denial

`DenyReason` is the operator's record. The sentence an acting agent reads is
the library's, not the host's, and is bound by
[ADR 0009](../adr/0009-a-refusal-renders-what-the-caller-already-holds.md),
which settles the question this spec previously left open — whether the host
should collapse a reason before rendering it to a model, and which reasons must
collapse.

The rule is that a denial renders a sentence of its own only when what it
discloses is something the caller already holds: the policy it was gated under,
the shape of the request it sent, or its own identity. A denial that turns on
whether a *named other* exists or resolves renders one shared sentence, so a
caller cannot learn a roster by reading which refusal came back. Applying it to
the enumeration above: `Disabled`, `MalformedRequest`, `UnclassifiedAction`,
`PolicyDenied`, `RememberedRefusal`, `NoRule` and `UnknownActor` may be worded
apart — a caller cannot vary its own actor id, so that variant is no more
probeable than `NoDispatchReason::SourceInactive` — while `UnresolvableApprover`
and `NoApprover` turn on a named approver and share one sentence.

ADR 0009 also fixes the mechanics: `Display` carries the agent's sentence and
`Debug` the operator's variant, because the safe rendering has to be the one
`{}` reaches for; the classification is per variant, by its most disclosing
path; and it is pinned by a wildcard-free `match` in a test, so a variant added
later does not compile until its author has classified it. This module supplies
that table and those tests when it lands.

## Invariants and constraints

- **Approval decides, never enacts.** No decision variant performs, schedules,
  or enqueues anything.
- **`approve` is total.** It cannot return an error, so it cannot be bypassed
  by returning one. See ADR 0008.
- **Fail closed.** Every path that does not produce a definite `Allow` produces
  `Deny` or `Ask`; `DefaultVerdict` cannot spell `Allow`; an absent policy, an
  unparsable action, an unknown actor and an unresolvable approver all deny.
- **Consent is not retroactive.** `(grant.granted_at_epoch,
  grant.granted_at_sequence) <= (request.epoch, request.sequence)`, always.
- **Refusals are epoch-local; grants are not.** `refusal.epoch ==
  request.epoch`; a grant survives an epoch advance until it expires.
- **Deny beats allow**, over rules, refusals and malformed input alike.
- **`Ask` names exactly one person**, never an agent and never a list.
- **A grant is never widened.** Coverage is a containment test on the scope
  key: `Action` covers `Call`, `Resource` covers only paths lexically inside
  its root, and nothing covers a different actor.
- **Order independence.** `approve` is a fold: permuting `grants` or
  duplicating an entry does not change the decision.
- **No clock, no storage, no host type, no callback, no async, no new
  dependency, in `crates/tinyhivemind-core::approve`.** `now` and `epoch` are
  arguments; grants, refusals and audit rows are host-owned. The necessarily
  async `ApprovalGate` port lives in `crates/tinyhivemind` instead, per the
  pure-fold/waiting split in [`AGENTS.md`](../../AGENTS.md).

## Acceptance criteria

- Every `DenyReason` variant has a test that produces it. This module adds no
  variant to the crate-wide `Error`, so the repository's per-error-variant
  coverage rule is discharged against `DenyReason` instead.
- The epoch invariant is pinned by a test that fails if the comparison is
  weakened to `granted_at_epoch <= request.epoch` alone.
- With `enabled: false`, `approve` returns `Deny { Disabled }` for every input
  in the fixture set, asserted rather than documented.
- Wire forms of `ApprovalDecision`, `ScopeKey`, `StandingGrant` and
  `ApprovalPolicy` are pinned, including `ScopeKey::render()`'s exact,
  collision-free byte layout.
- Every `DenyReason` variant is classified against ADR 0009 by a wildcard-free
  `match`, and the reasons that turn on a named approver render one identical
  sentence.

## Testing

The full one-test-per-failure-path list, including approver-resolution,
grant-basis and encoding coverage added after review, lives in
[`approval-testing.md`](approval-testing.md) so this spec stays under the
per-file line budget.

## Open questions

- **Should `Ask` be able to name a desk's operators as a fallback when the
  named approver is absent?** It would trade a deny for a broader question. The
  spec currently denies, on the grounds that picking a second person to ask is
  a policy choice the host can make by supplying a different `ApproverRule`.
- **Does the sequence half of the epoch fence need a host-visible ordering
  contract?** It assumes request sequences and grant sequences are drawn from
  the same host-owned counter. A host drawing them from two counters gets a
  fence that compares numbers with no relationship, and the library cannot
  detect it.

Whether a `Deny` reason may reach the acting agent, and which reasons must
share one sentence, is no longer open. It is settled by
[ADR 0009](../adr/0009-a-refusal-renders-what-the-caller-already-holds.md); see
[Rendering a denial](#rendering-a-denial).
