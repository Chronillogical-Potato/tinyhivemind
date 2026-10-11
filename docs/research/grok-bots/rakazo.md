# Rakazo

## What it is

- **URL** — <https://github.com/elie222/rakazo>
- **SHA read** — `d5f809efa5046fa476105d7357921400e94ae500` (2026-09-06)
- **Licence** — Apache-2.0 (`LICENSE`, `package.json:6`)
- **Size** — 1,230 tracked files, ~224k lines; a pnpm/Turbo TypeScript monorepo.
  `apps/{api,worker,web,desktop,mobile,www}`, `packages/{core,contracts,db,adapters,adapter-kit,memory,chat-ui,…}`.
- **Stack** — Hono + oRPC API, PostgreSQL via Prisma, Graphile Worker for jobs,
  React 19 web / Electron desktop / Expo mobile, "Pi" as the agent runtime.

Self-hostable persistent-agent platform: a user creates *bots* in a *space*;
each bot has its own chat thread, memory, scheduled routines, a sandboxed
"computer", and tools. Bots can message each other and spawn children.

## Architecture

The layering is deliberate and maps closely onto tinyhivemind's own split:

| Layer | Package | Role |
| --- | --- | --- |
| Wire types | `packages/contracts` | zod schemas, oRPC contract, ids, domain constants |
| Decision logic | `packages/core` | pure functions over caller-held data (mentions, projection, run FSM, cron, visibility) |
| Ports | `packages/adapter-kit/src/interfaces.ts` | 20+ provider interfaces, all typed against local types |
| Adapters | `packages/adapters` | Docker/E2B/Daytona/Box sandboxes, Pi runtime, memory, messaging, realtime |
| Storage | `packages/db` | Prisma schema + the transactional write helpers |
| Hosts | `apps/api`, `apps/worker` | the composition roots that own transactions and jobs |

`packages/adapter-kit/src/registry.ts:21-39` is a plain slot registry
(`runtime`, `sandbox`, `memory`, `home`, `artifacts`, `secrets`, `jobs`,
`realtime`, `models`, `connector`, `runner`, `voice`, `web`, `browser`,
`cloud-agent`); the composition root registers one adapter per slot and every
call site resolves by slot name.

`AGENTS.md:4` states the guardrail explicitly: "Vendor SDKs, configuration, and
translation belong only in adapters and composition roots." The rule is largely
honoured — but see the purity notes at the end.

## Agent identity, persistence, and lifecycle

An agent is a **row**, not a process. `Bot` (`packages/db/prisma/schema.prisma:314-367`)
carries identity (`name`, `title`, `description`, `instructions`, `color`),
placement (`spaceId`, `sectionId`, `position`, `pinned`), lineage
(`parentBotId`, `spawnKey`, `@@unique([spaceId, spawnKey])`), and per-agent
model preference (`modelProvider`, `modelId`, `thinkingLevel`).

What "persistent" actually means here — four independent durable surfaces, none
of them a running process:

1. **One thread per bot**, `Thread.botId @unique` (`schema.prisma:435-437`) — a
   durable transcript that survives every restart.
2. **A memory store** — `MemoryDocument` / `MemoryRevision`
   (`schema.prisma:792-825`), markdown documents scoped `bot` or `user`, with
   revision history.
3. **An agent home** — `AgentHome` (`schema.prisma:826-838`) holds a `revision`
   string; `AgentHomeStore.checkout/commit/restore`
   (`adapter-kit/src/interfaces.ts:248-266`) gives the bot a filesystem that is
   checked out into a sandbox and committed back after a turn.
4. **A computer** — `Computer` (`schema.prisma:854-885`), reconnected by
   `homeKey`/`scopeKey`, with lease fences for control and execution.

Lifecycle is `spawnBot` / `archiveSpawnedBot` / `archiveBot` / `destroyBot`
(`packages/adapters/src/child-bots.ts:37,208,258,332`). Archiving is a soft
`archivedAt`; deletion leaves a `BotDeletion` tombstone recording whether
memories were preserved (`schema.prisma:388-399`). A spawned child is idempotent
on `spawnKey` (`child-bots.ts:86-90`, nonce `spawn:${spawnKey}` at line 142).

Nothing runs between turns. All liveness is `Routine` rows with cron strings
(`schema.prisma:664-689`, `@@index([active, nextRunAt])`) drained by the worker.

## Conversation/session model and its schema

**Two logs per thread, both sequenced by the same owner row.** `Thread`
(`schema.prisma:435-458`) holds `nextEventSeq` and `nextMessageSeq` counters:

- `Message` (`schema.prisma:461-484`) — durable turns. `@@unique([threadId, seq])`,
  `@@unique([threadId, clientNonce])`, `blocks Json`, plus `botId`, `runId`,
  `replyToMessageId`.
- `Event` (`schema.prisma:486-504`) — the append-only activity log.
  `@@unique([threadId, seq])`, `type`, `payload Json`, `runId`.

Sequence allocation is an increment on the parent `Thread` inside the same
transaction as the insert (`packages/db/src/events.ts:1130-1153`), so seq is
gap-free and totally ordered per thread. Ordering is *per thread only*; there is
no space-wide sequence.

A thread belongs to **exactly one** bot or one group: `botId String? @unique`,
`groupId String? @unique` (`schema.prisma:436-440`). There is therefore no
single shared transcript that several agents read — there are N private
transcripts plus group threads, and cross-agent visibility is achieved by
*writing a copy* into the other thread (see dispatch, below).

Idempotency is `clientNonce`, used everywhere as a delivery key:
`send:${messageId}`, `bot-message:${key}`, `group-handoff:${runId}`,
`spawn:${spawnKey}`, `messaging-peer:${messageId}:${botId}`
(`thread-target.ts:596`, `bot-messages.ts:126`, `group-handoff.ts:26`,
`messaging-delivery.ts:196`).

`projectMessages` (`packages/core/src/events.ts:4-...`, 492 lines) is a **pure
fold** from the event log to `ThreadMessage[]`: durable `thread.message.created`
events push messages, `thread.progress` and `agent.tool.called` accumulate into
an ephemeral "live" projection keyed by run id, `thread.cleared` truncates
(lines 53-120). This is the closest analogue in the repo to tinyhivemind's
projection fold.

## What triggers a turn

A turn is a `Run` row (`schema.prisma:525-571`) with a `Task` holding the
prompt. `Run.trigger` is one of `user`, `bot_message`, `messaging`, `reaction`,
routine-driven, etc. The run FSM is pure: `packages/core/src/run-state.ts:12-43`
(`queued → leased → running → {waiting_input | waiting_takeover | terminal}`),
with `nextFence` at line 41 feeding the `leaseOwner`/`leaseFence`/`leaseExpiresAt`
triple on `Run`. Every state write in `packages/db/src/events.ts` is guarded on
matching `leaseOwner` + `leaseFence` (e.g. `events.ts:450-470`), which is how a
resumed or duplicated worker cannot double-write.

**Mention grammar** is pure and tiny (`packages/core/src/group-mentions.ts`):
`hasMentionToken` (line 12) matches `@Name` on Unicode-aware boundaries;
`resolveGroupTargetBotIds` (line 30) resolves a send.

**Fan-out is uncontrolled, and this is the sharpest contrast with tinyhivemind.**
`group-mentions.ts:44-50`: `@everyone` adds *every* member; otherwise every
member whose name is mentioned is added; line 52 falls back to the first member
if nothing matched. `apps/api/src/thread-target.ts:659-724` then loops over the
resolved ids and creates **one `Task` + one `Run` per target bot** in a single
transaction. One message therefore starts up to N turns. The only bounds are:

- `GROUP_MEMBER_MAX = 6` (`packages/contracts/src/domain.ts:73`), so N ≤ 6;
- if a target bot already has an active run, the message becomes a
  `SteeringMessage` injected into that run instead of a new one
  (`thread-target.ts:688-693`; schema at `schema.prisma:573-589`,
  `@@unique([messageId, botId])`);
- `cancelSupersededQueuedRuns` (`thread-target.ts:204-228`) cancels older queued
  user-triggered runs for the same bots, so a fast typist queues one turn, not five.

**Bot-to-bot dispatch is hop-bounded**, exactly the tinyhivemind idea:
`BOT_MESSAGE_MAX_HOPS = 6` with `nextBotMessageHop` / `botMessageHopExhausted`
(`packages/core/src/bot-messages.ts:14,34,38`) — pure, and the comment at
lines 9-13 states the reason ("nothing stops two bots replying to each other
forever; a person's own message always starts a fresh chain at hop 0").
`messageBot` (`packages/adapters/src/bot-messages.ts:62-290`) resolves a target
by id-then-name (`resolveBotAddress`, `core/src/bot-messages.ts:56`), refuses
self-messaging, refuses an exhausted hop (line 114) with a *carve-out* for a
terminal `result`/`status` reply back to the sender, then in one transaction
writes a `bot_message_sent` block into the sender's thread and a
`bot_message_received` block into the recipient's thread and creates exactly one
run there. `handoffToGroupBot` (`group-handoff.ts:15-213`) does the same inside
a group and additionally refuses handing a stage straight back to its sender
(line 104).

**Scheduler**: `JobPublisher` / `JobWorkerHost` ports
(`adapter-kit/src/interfaces.ts:237-246`) with a Graphile Worker adapter and an
in-memory one (`packages/adapters/src/wakeup.ts:11,45,105`). Cron evaluation is
pure (`packages/core/src/cron.ts:166-230`, `croner`).

## Model/provider selection

The decision is a **pure function**, `selectConfiguredModel`
(`packages/adapters/src/model-selection.ts:7-40`), over five inputs already
held by the caller: the bot's override, the credential for that override, the
default credential, space settings, and the deployment default. Two design
points worth stealing:

- "The override provider, model and credential must win together" (line 20):
  an override without a usable credential is discarded *whole*, and its
  `thinkingLevel` is dropped with it (lines 36-39) rather than leaking onto the
  fallback model.
- Secrets are resolved **after** the winner is known (`executor.ts:641-647`),
  not before — the selection function never touches a key.

Granularity: the *choice* is per-bot (`Bot.modelProvider/modelId/thinkingLevel`),
but it is **re-evaluated per run** (`executor.ts:1120-1127`) and the answer is
recorded on the `Run` row (`Run.modelProvider`, `Run.modelId`,
`schema.prisma:536-537`), so history stays truthful when the bot's preference
later changes. There is no per-task or per-difficulty routing and no
capability-based selector — nothing resembling tinyhivemind's responder ladder
or expert-delegation directory. `featuredModelProviders`
(`packages/core/src/model-providers.ts:16-32`) is UI ordering only.

## Sandbox abstraction

`SandboxProvider` (`packages/adapter-kit/src/interfaces.ts:78-155`) is the
largest port: `provision` → `prepare` → `execute` / `observe` / `act` /
`connectScreen` / `sendInput` / file IO / `exportWorkspace` / `importWorkspace` /
`snapshot` / `stop` / `destroy`, plus an optional `pageBrowser`.

What crosses the boundary is only local, serialisable data:

- `ComputerRef { id, botId, kind, providerRef, fresh? }` (`types.ts:55-62`) —
  an opaque handle; `fresh` tells the caller the provider made an empty
  replacement rather than reconnecting state.
- `CommandRequest { argv, cwd, env, pty, timeoutMs }` (`types.ts:63-71`) and a
  `ProcessEvent` stream (`stdout`/`stderr`/`exit`).
- `PortableFile { path, content, executable }` (`types.ts:49-53`) — the
  migration format between providers.
- `ComputerInput` / `ComputerAction` (`types.ts:88-102`) — a closed union of
  key, pointer, clipboard, scroll, wait, open, launch.
- `AdapterContext` (`types.ts:3-19`) — the ambient per-operation envelope:
  `operationId`, `traceId`, `spaceId`, `userId`, `botId?`, `runId?`, an
  `AbortSignal`, a `screenLeaseId` fence, and connected connectors.

The split of `provision` from `prepare` is a deliberate crash-safety move
("Allocate or reconnect the computer, returning its reference before fallible
setup", line 86) so a crash mid-setup still leaves a recorded ref to reclaim.
Concurrency on a shared computer is fenced in the database:
`Computer.controlFence` / `executionFence` and a `ComputerExecutionLease` with
`@@unique([computerId, botId])` (`schema.prisma:854-901`). Implementations:
Docker, E2B, Daytona, Box, desktop, `none`, plus a `fake-sandbox` and a shared
`sandbox-conformance.test.ts` every provider must pass.

## Multi-client sync

One durable ordering, one soft signal:

1. Every mutation appends an `Event` with a thread-scoped `seq` in the same
   transaction as the state change (`packages/db/src/events.ts:1130-1153`).
2. Realtime is only a **wake hint**: `notifyRealtime` publishes `{cursor}` on a
   thread topic and swallows every failure (`events.ts:1155-1161`); the call
   sites log and continue, commented "The event is durable; subscribers recover
   it from their persisted cursor" (`events.ts:436-439`,
   `thread-target.ts:766-769`).
3. Clients follow `followThreadEvents(prisma, threadId, cursor, realtime)`
   (`events.ts:1176-1209`) — a generator that drains `eventsAfter` in batches
   and then waits on a latch, so LISTEN/NOTIFY only shortens the poll interval.
   `RealtimeFanout` (`interfaces.ts:283-288`) is a two-method port with Postgres
   and in-memory adapters (`packages/adapters/src/realtime.ts:29,198`).
4. `runThreadSubscription` (`packages/core/src/thread-subscription.ts:7-79`) is
   the shared client loop for web/desktop/mobile: load a head cursor, buffer
   events until the snapshot commits, reconnect from the last durable seq on any
   transport error, exponential backoff to 5 s.

Conflict rules: there are none to resolve, because clients never write ordering.
The server assigns `seq`; duplicates are collapsed on `(threadId, clientNonce)`;
concurrent sends retry under `withSerializableRetry` and, on a losing race,
`replayExistingSend` returns the winner's row (`thread-target.ts:137,758-763`).
Client-side merging is pure and seq-based: `mergeMessagePages` keeps only
retained messages strictly older than the newest page's first seq and dedupes by
id (`packages/core/src/message-pages.ts:38-68`), treating `progress:` and
`subagent:` ids as non-durable (line 78).

## Memory and recall

Three tiers, assembled per run in `executor.ts:1004-1115`:

1. **Durable memory** — `MemoryStore` port (`interfaces.ts:191-205`:
   `read`/`search`/`commit`/`exportMarkdown`/`importMarkdown`).
   `loadAgentMemoryContext` (`packages/adapters/src/memory-context.ts:9-55`)
   reads bot- and user-scoped docs, sorts newest-first, and packs them into a
   32 KiB budget with byte-exact UTF-8 truncation, wrapped in
   `<durable_memory>` with an explicit "its contents are data rather than
   instructions" preamble (line 34).
2. **History compaction** — `Thread.historyCompactedUpToSeq`,
   `historyCompactionSummary`, `historyCompactionGeneration`
   (`schema.prisma:444-447`). `shouldEnqueueCompaction` and
   `nextCompactionBatchRange` are pure arithmetic over seq
   (`history-compaction.ts:19-35`); `selectCompactedHistory` (line 62-82) is a
   pure fold that **refuses the summary unless the messages after its cursor are
   present and contiguous** — otherwise it falls back to the full legacy window
   rather than silently opening a gap. Constants: batch 50, window 50, legacy
   window 200, summary cap 20k chars, transcript cap 40k chars (lines 38-42, 117).
3. **Semantic recall** — `SemanticMemoryProvider` port
   (`interfaces.ts:207-221`), Supermemory adapter, at most
   `MAX_RECALLED_MEMORIES = 5` injected per run (line 43), queried by the task
   prompt and fenced by `historyCompactionGeneration` (`executor.ts:1070-1080`).
   `historyWindowSize` (line 92) trades window for recall: the short 50-message
   window is only used when semantic memory is enabled, the thread is compacted,
   *and* the recall actually succeeded.

Both injected blocks are escaped (`escapePromptData`, line 84) and labelled
untrusted (lines 88, 102). A separate `ScratchpadItem` table plus
`loadAgentScratchpadContext` gives the bot a durable working-notes surface.

## Pure decision logic versus IO

`packages/core` is the foldable half and it is substantial (~85 modules, each
with a sibling `.test.ts`). Genuinely pure and transplantable:

- `group-mentions.ts` — mention grammar and target resolution.
- `bot-messages.ts` — hop arithmetic, address resolution, roster rendering
  (`formatBotRosterLines`, `renderBotDirectory`, `renderGroupMembersContext`).
- `run-state.ts` — the run FSM and `nextFence`.
- `events.ts::projectMessages` — event log → messages.
- `message-visibility.ts` — `userVisibleMessages` drops peer-run activity and
  optionally keeps receipts as chips (lines 25-41).
- `message-pages.ts`, `thread-message-updates.ts` — client merge folds.
- `cron.ts`, `search.ts`, `action-approval.ts`, `secrets-guard.ts`.
- In `packages/adapters` but still pure: `model-selection.ts`,
  `history-compaction.ts:19-115`.

IO lives in `packages/adapters` behind the `adapter-kit` ports, with
transactions owned by `packages/db` helpers and `apps/api` / `apps/worker`.

**Purity is not enforced, and it has leaked.** `packages/core/package.json`
depends on `dotenv`, `croner`, `@observablehq/plot` and `d3-dsv`, and the
package exports `./node/load-root-env` and `./node/screen-proxy-response` — a
Node-only env loader and an HTTP response helper shipped from the "pure"
package. There is no equivalent of `assert-pure.sh`; the boundary is a prose
rule in `AGENTS.md:4` only.

## README claims worth flagging

- "Persistent bots with their own conversations, memory, routines, and history" —
  accurate; all four are tables, and nothing is a resident process.
- "Bots that can delegate to peer bots or short-lived subagents" — accurate
  (`bot-messages.ts:62`, `child-bots.ts:37`), but the delegation is
  fire-and-forget with a 6-hop ceiling, not a bounded child turn with a single
  returned answer; the "return" path is a best-effort `returnBotMessageOutcome`
  (`bot-messages.ts:318`).
- `MemoryStore.describe()` advertises `capabilities.search: true`
  (`packages/memory/src/index.ts:23`), but `search` is a
  `content.toLowerCase().includes(query)` scan over every document with a
  constant `score: 1` (lines 48-67). Semantic search is a *different*,
  optional port.
- "Shared Team Computers and isolated Private computers" — real, but sharing is
  serialised by a database lease, so two bots cannot execute on one computer
  concurrently (`schema.prisma:886-901`).

## Mechanism → tinyhivemind has → does not have

| Mechanism in Rakazo | tinyhivemind has | tinyhivemind does not have |
| --- | --- | --- |
| Pure mention grammar (`@Name`, `@everyone`), Unicode boundaries | Yes — richer grammar with `@desk`, `@this`, normalization (`docs/specs/mentions.md`) | — |
| `@everyone` fans out to N runs in one transaction (`thread-target.ts:659-724`) | Deliberately refused: one message, one turn | — (anti-pattern; the bound is only `GROUP_MEMBER_MAX = 6`) |
| Hop-bounded bot-to-bot dispatch, `MAX_HOPS = 6` | Yes — hop-bounded, idempotent dispatch (`mention-dispatch.md`) | — |
| Idempotent delivery keyed on `(threadId, clientNonce)` | Yes — atomic host enqueue contract | The *naming convention* for delivery keys per call site |
| Steering: a message to a busy agent joins the running turn instead of starting one | — | A "turn in flight" concept; mid-turn message injection |
| Run FSM + monotone lease fence guarding every state write | Partly — the responder ladder decides, the host waits | A published run/turn state machine with fence semantics as a pure fold |
| Event log → message projection as a pure fold | Yes — the projection fold is the core of `tinyhivemind-core` | — |
| Two logs per surface (`Message` + `Event`) with independent seq counters | Deliberately refused (one host-owned sequence, no second journal) | — |
| Peer visibility as "context only" vs "wake" (`messaging-delivery.ts:200-224`) | Yes — visibility filter on the projection, separate from dispatch | — |
| Model selection as a pure fold over bot/credential/space/deployment, resolved per run and recorded on the run | Yes — `Selector` port and `responders.md` | Override-with-credential-must-win-together; recording the chosen model on the turn record |
| Sandbox port with `provision`/`prepare` split and `PortableFile` migration | — (out of charter) | Any execution-environment port |
| Realtime as a soft hint over a durable cursor; clients recover from seq | Yes — caller-owned watermarks, stateless deltas (`continuous-sharing.md`) | The client-side reconnect loop (host concern) |
| Client merge folds keyed on seq, non-durable id prefixes | Partly — projection covers the server half | A published client-side page-merge algebra |
| Contiguity check before trusting a compaction summary | Partly — `recall.md` covers ranking and budget | The "refuse the summary if the tail is not contiguous" invariant |
| Byte-budgeted, newest-first memory packing with untrusted-data framing | Yes — stated per-message budget in `recall.md` | Explicit prompt-injection framing of injected context as data |
| Semantic recall fenced by a compaction generation counter | — | Generation-fenced retrieval |
| Roster/directory rendering for the prompt (`renderBotDirectory`) | Yes — roster records | — |
| Group handoff refusing to return a stage to its sender | Partly — cross-desk referral is one bounded child turn | An explicit "do not hand back" edge rule |
| Deliberation: traces, salience, quorum, cross-inhibition, attention market | Yes — core's `hive` module | — (Rakazo has no deliberation mechanism at all; a group is N independent bots) |
| Enforced purity boundary in CI | Yes — `assert-pure.sh` | — (Rakazo's `packages/core` has leaked `dotenv`, `croner`, plotting) |
