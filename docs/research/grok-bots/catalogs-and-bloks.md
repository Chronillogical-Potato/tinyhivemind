# Grok Bot catalogs and Bloks — pattern mining

Sources:

- `RongleCat/awesome-grok-bot`, commit `76ce421dd90290ac0ca311d2837f90cf174a4132`
  (cloned shallow 2026-09-06). Licence: **CC0** (per README badge). ~580 curated
  entries across official resources, tutorials, field cases, skills/plugins/MCP,
  open-source alternatives, and community/failure-mode threads.
- `majiayu000/awesome-grok-bot`, commit `ec712e85a2e92b98f3d1c45ced4ff0e9674ad61a`
  (cloned shallow 2026-09-06). This list turned out to be overwhelmingly a
  catalog of individual *published Bot templates* (x.ai/bot share links —
  "2nd Brain," "Ads Operator," etc.) rather than architecture or tooling
  repos; it contributed almost nothing distinct from RongleCat's list for
  this survey's purpose and is not quoted further below except where noted.
- `hamedgitty/bloks`, commit `76ce421dd90290ac0ca311d2837f90cf174a4132`
  (cloned shallow 2026-09-06). Licence: **FSL-1.1-MIT** (Functional Source
  License — converts to MIT after a time delay; source-available, not OSI
  MIT at time of reading).

Per the brief, individual catalog entries are not summarized exhaustively.
This file extracts recurring **architectural patterns** across the ~580-entry
list, a **shortlist of repos judged relevant to shared-transcript multi-agent
mechanics** (as opposed to app templates, installers, or usage-quota
dashboards — the large majority of the list), and **source notes on Bloks**,
which was read directly since it is the one target here that is actual
runnable multi-agent session code.

---

## Part 1 — Recurring patterns mined from the two awesome-lists

The ~580 entries in RongleCat's list cluster into a small number of shapes
that recur across dozens of independently-built repos. None of these are
read as authoritative — they are what many unrelated builders converged on
when facing the same constraints (one shared cloud computer, per-account
rather than per-Bot isolation, official docs explicitly warning "Bots are
not a security boundary" — every Bot on an account sees the same logins and
files).

1. **Chief-of-Staff / layered hierarchy.** Repeated verbatim as an
   architecture across many independent tools: `botops` (CoS → L2 → L3
   layers, "quiet swarm" rules), `3Fold-Labs/grok-bot-org-chart` (permissions
   playbook with CoS/Eng/Ops role templates on one shared computer),
   `mandarwagh9/botroster` (named teammates + one durable computer +
   approvals + routines, in Rust/Tauri). This mirrors the orange book's
   pattern 1 closely enough that it looks like the dominant community mental
   model, not one author's invention.

2. **Ticket/job-board handoff instead of raw chat.** `pmcclelland/bot-board`
   (shared kanban — Backlog/To Do/Doing/Done — with a REST + single MCP
   URL surface for humans and Bots together), `Archive228/foreman` (crew
   harness with an *effect ledger* and shift reports against declared work
   packs). Same shape as the orange book's "handoff is a ticket, not a chat
   line" observation, but here it's externalized into shared infrastructure
   (a board or ledger) rather than living inside one Bot's memory.

3. **File-driven inbox / crew protocols as a portable primitive.**
   `nescafe2009/dsh-grokbot` implements "always-on named crews with per-bot
   workspaces and a file-driven inbox protocol" explicitly designed to be
   *compatible with a different host* (`todi-hub`) — i.e., someone
   independently arrived at "a filesystem inbox is a good host-agnostic
   transport for cross-agent messages," which is one plausible concrete
   shape for the kind of port `tinyhivemind`'s host interface abstracts over.

4. **Approval queues as first-class UI, not a side effect.** Distinct from
   the orange book's auto-review cards, several tools make the approval
   queue *itself* the product surface: `pacifico-1106/grokbot-control-plane`
   (ID cards, approval queues, an audit timeline, aimed at Japanese SMEs
   treating Bots as named employees), `RishabhPatel123/Rixy-Bot` (an Android
   multi-agent framework with remote execution, swarms, and *push* approval
   queues rather than pull/poll). The recurring idea: approval state needs
   to be queryable and orderable on its own, independent of any one Bot's
   transcript.

5. **Session/control-plane sidecars that watch or drive a live agent
   session from outside it.** `pyrex41/huginn` (a host sidecar letting a
   Grok Bot attach to live Claude Code / Codex / Grok Build sessions via a
   native control-plane MCP exposing list/watch/prompt/interrupt),
   `AstroQore/auspex` (a native Mac app watching live coding-agent sessions
   including Grok Bot/Grok Build, surfacing transcripts and permission
   waits), `Dahhrk/grokbot-fleet` (an MCP server that lists/updates Bots —
   prompts, routines, skills — because there is no official fleet-management
   API). All three independently reinvent "a read/watch/interrupt port over
   someone else's session," which is structurally the same shape as a
   `SessionLog`/`Selector` port in `tinyhivemind`.

6. **Handover ledgers as an explicit safety mechanism, not just an audit
   log.** `Nesqual-Tech/nesqbot` (isolated Linux desktops, a real browser,
   "human gates before consequential actions," and *handover ledgers*
   specifically) frames the ledger as part of the handoff contract between
   agents — closer to pattern 2's ticket idea but framed around trust
   transfer rather than task tracking.

7. **"Bots are not a security boundary" as a load-bearing community
   finding.** A Cursor forum thread with that exact title (linked in the
   community/failure-modes section) states every Bot on an account shares
   logins and files — there is per-*user* isolation (a Firecracker microVM
   per user, per the official `docs.x.ai/grok-bot/security` page linked in
   the list) but not per-*Bot* isolation within that VM. This is the single
   most consequential architectural fact in the whole catalog for anyone
   modeling multi-agent trust boundaries: "who is on a desk" and "what can
   this agent do" are two different questions, and Grok Bot conflates them
   at the account level. `tinyhivemind`'s charter deliberately keeps
   membership (desks/roster) and capability (host-owned) as separate
   concerns — this finding is independent validation that the split is the
   right one, not an academic nicety.

8. **Loop-prevention and hop-bounding are inconsistently handled across the
   ecosystem.** Only a minority of the surveyed repos mention any bound on
   agent-to-agent chains (see the shortlist below and the Bloks notes in
   Part 3, which does bound it explicitly as `MAX_AGENT_HOPS`). Most
   catalog entries describe "Bots message each other" as a feature with no
   stated ceiling, which is exactly the failure mode `tinyhivemind`'s
   "one message, one turn" rule and mention-dispatch hop bound are built to
   foreclose.

---

## Part 2 — Shortlist: repos relevant to shared-transcript multi-agent mechanics

Filtered down from ~580 entries to the ones that describe genuine session/
transcript/dispatch mechanics rather than an app template, installer,
packaging repo, or usage-quota widget. One line of reason each; none were
cloned or read beyond their catalog description, so treat these as leads for
further reading, not verified findings.

| Repo | Why it's relevant |
|---|---|
| `pyrex41/huginn` | A control-plane MCP (list/watch/prompt/interrupt) attaching to live third-party agent sessions — the closest analog in the wild to a `SessionLog`/`Selector` port boundary. |
| `AstroQore/auspex` | Independently built session-watching UI (transcripts + permission waits) across multiple agent runtimes — a second data point for what a "projection for one viewer" needs to expose. |
| `Dahhrk/grokbot-fleet` | MCP server managing a Bot fleet in the absence of an official management API — real-world evidence of what a roster/registration port needs to cover. |
| `nescafe2009/dsh-grokbot` | File-driven inbox protocol explicitly designed to be host-agnostic (compatible with `todi-hub`) — a candidate concrete transport shape for cross-agent messages. |
| `Nesqual-Tech/nesqbot` | Handover ledgers plus human gates before consequential actions — a trust-transfer framing of the handoff-artifact pattern, distinct from a plain kanban. |
| `pmcclelland/bot-board` | Shared kanban with one MCP surface for humans and Bots together — externalizes the ticket/handoff pattern as shared infrastructure rather than in-memory state. |
| `Archive228/foreman` | Crew harness with an explicit *effect ledger* and shift reports against declared work — closest catalog analog to an audit trail over agent actions. |
| `granda/botops` | CoS → L2 → L3 layered fleet lifecycle with stated "quiet swarm" rules — a named hierarchy-depth convention worth comparing against the `hive` module's cross-inhibition/quorum design. |
| `RishabhPatel123/Rixy-Bot` | Push (not poll) approval queues plus swarms in a from-scratch Android multi-agent framework — a second independent implementation of the approval-queue-as-first-class-surface pattern. |
| `mandarwagh9/botroster` | Named teammates, one durable computer, approvals and routines, built explicitly as an open alternative — a compact reference for what "roster" + "desk" means to another implementer. |

---

## Part 3 — Bloks source notes

`hamedgitty/bloks` is a shipping, source-available Electron/desktop app (not
just a design doc) implementing a local-first Grok-Bot-shaped multi-agent
chat. Because it is real running code with committed design-rationale
comments, it is the most directly comparable artifact to `tinyhivemind` found
in this survey — close enough that several of its choices are useful as
**negative** evidence (what happens when the invariant `tinyhivemind`
enforces is *not* enforced) as well as positive inspiration.

### Agent/session model, briefly

- **Bot record** (`server/store.ts`): identity (name/title/color/shape),
  a `seniority` (1–5) used for turn ordering and tie-breaking in a room, a
  `computer` scope (`cloud | sandbox | local | off`), an `approvals` mode
  (`ask | edits | auto`, with deny rules always outranking mode), and a list
  of `tasks` — each task (`TaskRecord`) is one *lane*: its own transcript id,
  its own provider session cursor, its own busy flag. A Bot runs at most
  `MAX_TASKS = 3` lanes concurrently.
- **Message** (`server/store.ts`): tagged by `kind` (`text | options |
  activity | screen | notice | artifact | connector | secret | component`),
  optionally has `from` (which agent spoke, absent in 1:1 chats where it's
  unambiguous), `replyTo` (quoting an earlier message), `reactions` (agent
  or user emoji reactions), and a `deleted` flag that keeps the row (so
  quoted replies still resolve) while blanking the text.
- **Room** (`server/bloks.ts`, called a "blok"): a `memberIds` list plus a
  `leadOnly` flag. `addressees(text, members)` is the mention-resolution
  function: longest-name-first matching, case-insensitive, each match
  consumed from the text so `@Bobby Tables` can't be mistaken for `@Bo`; a
  bare message with no `@name` resolves to **the whole room** (`mentioned:
  false`, `ids: members.map(...)`) — this is a genuine broadcast, not a
  bounded fan-out.
- **Turn ordering in a room** (`server/index.ts`, `speakInTurn`): when
  several members are addressed by one message, each takes a *full,
  sequential* turn, ordered by ascending `seniority` — juniors answer
  first, the most senior member answers last and can review/adjudicate
  what came back. This literally implements "the most senior one speaks
  last and makes the call when members disagree" from the README.
- **Hop bounding** (`server/index.ts`, `MAX_AGENT_HOPS`, `agentHops` map):
  agent-triggered turns increment a `hops` counter; once `hops >
  MAX_AGENT_HOPS` a further agent-to-agent message is silently dropped.
  This is Bloks' actual answer to the "two agents talk forever" failure
  mode the orange book's group-chat pattern doesn't address at all — but
  note it bounds the *chain length*, not the *fan-out per inbound message*
  (a bare message to an unaddressed room still starts as many turns as
  there are room members).
- **A message from an agent is scoped differently from a message from a
  user**: an agent's message only reaches whoever it explicitly named
  (`mentioned ? ids : []`), *except* a `toAll` "kickoff brief" flag meant
  for the whole room. A human's bare message defaults to the whole room; an
  agent's bare message defaults to reaching nobody. This asymmetry is a
  deliberate anti-runaway-broadcast measure for agent-originated messages
  specifically, while still allowing a human-originated broadcast.
- **Team formation as a proposal, never automatic** (`server/teams.ts`): an
  agent can end a reply with a fenced `bloks-team` JSON block proposing 2–4
  new specialist members with skills; this is parsed, validated (name/title/
  description length caps, `MAX_HIRES = 4`, minimum 2 members), and
  surfaced to the user as a card. Spawning agents spends the user's tokens,
  so the user's approval — never the model's own say-so — is what actually
  creates the room and the agents. This is the most concrete instance found
  anywhere in this survey of "delegation requires an approval in the
  loop," matching `tinyhivemind`'s core worry about `@everyone` starting N
  turns "without an approval in sight."
- **Job board** (`server/jobs.ts`): a second, independent dispatch
  mechanism alongside room mentions — a `Job` is posted "to nobody in
  particular"; a plain word-overlap scoring function (`scoreAgent`, no
  model call) ranks candidate agents by title/brief relevance; the
  highest-ranked agent is *offered* the job and may decline
  (`readClaim` parses a reply for an explicit pass marker), in which case
  it goes to the next-ranked candidate. The file's own comment states the
  rationale plainly: ranking is cheap, cannot itself fail, and a bad
  ranking only costs one wasted turn (an offer the agent declines) rather
  than a wrong answer. This is a pure-fold-shaped mechanism: score, rank,
  offer-or-decline, with no LLM call in the routing decision itself.
- **Engine-switch freshness** (`server/turn-context.ts`): if the model/
  engine backing a lane changes between turns, the new engine either has no
  session (sees a bare message with prior context missing) or an old
  session-cursor that predates the switch. `engineIsFresh` detects this and
  `freshTurnText` inlines a full transcript replay into the new engine's
  first turn rather than trusting a resumed cursor across an engine change.
  This is a narrow but concrete answer to a projection problem
  `tinyhivemind`'s spec space (`sessions.md`, `continuous-sharing.md`) also
  has to solve: what does "this participant's view of history" mean when
  the thing consuming that view changes mid-stream.
- **Workflows as disk-resident state machines** (`server/workflows.ts`): a
  workflow is a trigger + steps + an optional human-approval step. Its
  central design decision, stated verbatim in the file's header comment: a
  run is *state on disk advanced by a tick*, never a promise or callback
  held open in memory — because an in-memory suspended run dies with the
  app, exactly when a pending approval has been sitting unanswered
  overnight. Deliberately excludes any network-calling step (to avoid an
  unbounded data-exfiltration surface on a trigger) and any regex in
  conditions (to keep the whole evaluator side-effect-free and boundable —
  "contains" covers what people actually write).
- **Tamper-evident ledger** (`server/ledger.ts`): an append-only,
  hash-chained log (each entry's hash depends on the previous entry's
  hash) covering approvals, agent lifecycle events, skill/routine/job/
  workflow/policy events, and "control taken/released" (see below). The
  file is explicit that this is *tamper evidence*, not *tamper-proofing* —
  anyone who can write the file can rewrite the whole chain; what they
  cannot do is edit one entry and have the chain still verify.
- **Policy engine** (`server/policy.ts`): deny-before-allow evaluation
  order (deny always wins, so a broad later allow can't silently reopen
  something explicitly shut), rules compare against the actual target the
  tool call carries (not a label, to prevent rename-to-bypass), rules
  cannot embed arbitrary logic (no expression language, so nothing can
  itself misbehave), an unrecognized operator is refused rather than
  ignored (fail closed inside a rule), but — the one deliberately
  non-strict default — an *empty* policy means "ask a human," not "deny
  everything," on the reasoning that this replaces a human decision, not a
  security gateway, so refusing everything by default the day someone
  opens the settings screen would be a worse product.
- **"Take the wheel" interrupt** (mentioned in README, wired through the
  ledger's `control.taken`/`control.released` kinds): a human manually
  driving something stops all pending agent turns outright — refused, not
  queued — specifically because a queued turn would replay a plan made
  before the human's manual changes, on a now-stale premise.

### What Bloks does *not* claim to solve

The README and code comments are candid about scope: Bloks is local-first
and single-machine (macOS desktop; the iOS companion is a remote viewer/
approver over a "sealed relay," not a second execution site), so it never
had to solve the problem `tinyhivemind` exists for — multiple independent
*hosts* reading and writing one *shared* transcript. Every mechanism above
(rooms, jobs, ledger, policy) is a single Node process's in-memory or
on-disk state; there is no host-port abstraction, no notion of "the host
owns storage and this crate holds none," and no host-type-free API surface
— all things the tinyhivemind charter enforces explicitly.

---

## Mechanism → tinyhivemind has → does not have

| Mechanism observed here | tinyhivemind has | tinyhivemind does not have |
|---|---|---|
| Bloks' `addressees()`: longest-name-first, span-consuming mention resolution, bare message = broadcast to non-agent authors | `docs/specs/mentions.md` already defines a mention grammar and normalization/resolution algebra with `@everyone` treated as a bounded list, not a broadcast | Bloks' specific greedy longest-match-with-consumption algorithm is worth comparing against tinyhivemind's own normalization rules for edge cases (e.g. one name being a prefix of another) |
| Bloks' seniority-ordered sequential turn-taking in a room (juniors first, senior reviews last) | Nothing analogous; `tinyhivemind`'s "one message, one turn" invariant forecloses a message ever starting *more than one* turn in the first place | No concept of "seniority" or a deliberate multi-turn review chain — by design, since the charter treats N-turns-from-one-message as the failure mode to avoid, not a feature to order |
| Bloks' `MAX_AGENT_HOPS` bound on agent-triggered chains | `mention-dispatch.md`: bounded one-target dispatch with a hop bound; `cross-desk-referral.md`: one bounded child turn, one answer returns | Bloks bounds chain *length*; it does not bound *fan-out* from one inbound message the way tinyhivemind's core invariant does — a useful negative data point |
| Bloks' job board: pure word-overlap ranking + offer/decline, no model call in the routing decision | `expert-delegation.md`'s transactive-memory directory (folded from grounded deposits and citations) is a comparable "who should get this" mechanism that is also a pure fold | No offer/decline handshake — delegation decisions in tinyhivemind resolve to a target rather than modeling a candidate's right of refusal |
| Bloks' team-formation-requires-approval gate before spawning agents | Matches the charter's core worry directly: "`@everyone` is a list, not a broadcast... a mention that could start N turns without an approval in sight is the failure mode the whole design avoids" | No host-facing "propose a team, get approval, then materialize desks/roster entries" flow — team/desk creation is assumed to be a host-side operation already, outside this crate's scope |
| Bloks' engine-freshness replay (`turn-context.ts`) for a lane whose backing engine changed | `sessions.md` and `continuous-sharing.md` cover host-owned paging and stateless attributed deltas, which is the same *kind* of problem (what does "this viewer's history" mean under a discontinuity) | No specific handling for "the thing consuming the projection changed identity mid-session" — tinyhivemind's discontinuities are about viewers and watermarks, not engine/model swaps |
| Bloks' hash-chained tamper-evident ledger over approvals and agent lifecycle events | Nothing analogous; audit/action-logging is explicitly host territory under "the host owns storage" | No append-only audit log of its own, by design — would need to be a host-implemented concern if ever wanted, never a second journal inside tinyhivemind (the charter explicitly forbids a second append-only log) |
| Bloks' deny-before-allow policy engine with fail-closed-on-unknown-operator, but ask-not-deny on an empty policy | Nothing analogous — approval/permission gating is host territory, not hive-mind mechanics | No policy/permission port at all currently; if one is ever added, Bloks' "empty policy means ask, never means deny-everything" default is a documented design tension worth deliberately deciding on, not defaulting into |
| The catalog's "Bots are not a security boundary" (per-user, not per-Bot, isolation) | The charter's clean separation of desks/roster (who's present) from host-owned storage/capability (what an agent can do) already keeps these as two questions | Nothing to add here — this is confirming evidence the existing split is correct, not a gap |
