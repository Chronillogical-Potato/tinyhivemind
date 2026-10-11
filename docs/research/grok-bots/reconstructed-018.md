# grok-bot-0.18-reconstructed — a survey for `tinyhivemind`

## What the project is

- Repository: <https://github.com/b-nnett/grok-bot-0.18-reconstructed>
- Commit read: `a9f633e09d49a85829b8236331b9e21f7e612634` (2026-08-23,
  "Document project and preserve original installers"), the only commit on a
  shallow clone of the default branch.
- Licence: **none.** There is no `LICENSE` file; `NOTICE.md` says "no upstream
  source-code license is asserted or granted here". Nothing here may be copied
  into `tinyhivemind` — this is a read-and-learn exercise only.
- Size: 2,111 files — about 177,000 lines of hand-written TypeScript under
  `source/` (a further 264,000 there are generated protobuf) plus 52,000 lines
  under `frontend/`. Eight `node --test` files, all packaging/publication
  regressions plus two router tests.

It is an unofficial, source-oriented reconstruction of the shipped Grok Bot
0.18.0 macOS app (Anysphere; upstream bundle id `com.anysphere.sand`, hence the
`sand`/`Sand` prefix on almost every identifier), recovered from the compiled
`app.asar`, so module boundaries and names are inferred. `PROVENANCE.md` sets an
"evidence-only reconstruction rule": recovered source may express only behaviour
supported by an inspectable artifact anchor. Thirteen modules are honest empty
stubs carrying a comment such as "No named runtime surface survived tree
shaking" (for example
`source/packages/agent-core/conversation-actions/steer-outbox.ts:1`). On top of
the reconstruction the author added four things of their own: an inference
router, routed MCP tooling, local usage counters, and an optional local Docker
sandbox.

## Architecture as found

Four processes, each a separate source root: `source/electron-main/` (desktop
lifecycle, settings, auth, box connectors, ownership of the coordinator child
process), `source/electron-preload/` (the trusted bridge to the renderer),
`source/node-agent-coordinator/` (the process that talks to inference
providers), and `source/host/` — the interesting layer: sessions, transcripts,
turns, groups, tools, permissions. The coordinator's control channel
(`source/shared/rpc/coordinator-port.ts`) is a five-variant frame union
(`lifecycle`/`request`/`cancel`/`reply`/`event`) with a pure parser,
`parseCoordinatorFrame` (line 35), returning `{accepted, frame}` or
`{accepted:false, rejection}` rather than throwing.

The host is assembled from 35 named extensions
(`source/host/extensions/extension-ids.generated.ts:1`) started through a tiny
DI graph in `source/internal/host-extensions.ts`. `defineHostExtension` (line 16)
takes `{id, dependencies, start}`; `resolveHostExtensionBootOrder` (line 57) is
a **pure topological sort** raising named errors for a duplicate id, a
self-dependency, a missing peer and a cycle. `startHostExtensions` (line 84) is
the impure half: it walks that order, injects peers' APIs, and unwinds teardowns
in reverse on failure — the pure/impure split `tinyhivemind` makes.

The renderer reaches the host over a flat command table:
`SAND_GATEWAY_COMMANDS` (`source/host/gateway-protocol.ts`) maps **122** string
command names onto methods of one `GatewayApi` object typed `any` (line 2) — a
dispatch switch, not a typed boundary.

## Agent identity and the roster

There is no single `Agent` type: identity is scattered across four persisted
surfaces per agent and reassembled on demand. The closest thing to a schema is
`AgentMetadata`
(`source/packages/agent-kv/agent-store.ts:40`), zod-validated at line 76 and
stored hex-encoded under the key `"metadata"` in the agent's SQLite `kv` table:
`agentId`, `latestRootBlobId`, `name`, `mode`
(`"default"|"plan"|"debug"|"search"`), `isRunEverything`, `approvalMode`
(`"allowlist"|"unrestricted"|"auto-review"`), `createdAt`, `lastUsedModel`,
`currentPlanUri`, `subagentInfo` (`parentAgentId`, `rootParentAgentId`,
`toolCallId`, `typeName`), `blobEncryptionKey`.

Alongside it: `profile.json` (`SandAgentProfile` — `name`, `description`,
`title`, `avatarShape`, `avatarColor`; `host/agents/agent-profile.ts:6`),
`settings.json` (`notifyOnAgentUpdates`, `hiddenFromSidebar`), and KV keys on
the DB (`unreadState`, `awaitingUserResponse`, `origin`, `purpose`,
`conversationPartners`; `session/agent-db.ts:62`).

**The roster is the filesystem.** `listAgents`
(`source/host/extensions/session/session-roster.ts:7`) does
`readdir(host.rootDir)`, treats every directory as an agent, `stat`s its
`store.db` and folds each into a summary via `buildSummary`
(`session-summaries.ts:16`), sorted by `updatedAt` descending. There is no
roster table, no roster record type and no handle namespace: the agent id *is*
the directory name, validated by `assertValidSandAgentId`. Caps are constants —
`MAX_AGENTS_PER_USER = 50`, `GROUP_MAX_MEMBERS = 6`
(`source/shared/agents/agents.ts:53`). `buildSummary` returns an anonymous
object: a roster entry's shape is nowhere declared, only its *input* is
(`DbExtras`, `session-summaries.ts:8`). The one genuinely pure piece is
`upsertAgentSummary` (`shared/agents/agent-summaries.ts:13`), an
array-in/array-out fold keyed on `.id`; everything around it is stateful, and
`RosterProjection` (`transcript/roster-projection.ts:25`) is an `EventEmitter`
with six mutable maps and a debounced flush despite the name.

Human versus agent is discriminated in only two places, both group-scoped.
`GroupMessage.speaker` is a tagged union of `{kind:"user", name?}` and
`{kind:"member", id, name}` (`source/host/groups/group-chat.ts:2`); cross-user
rooms are explicit, `RemoteRoomMember = {kind: "agent" | "human", authId,
agentId, displayName, avatarUrl?}` (`remote-room-store.ts:1`). A local group is
agents-only (`SandGroupConfig` is `{version, memberIds[], remoteMembers?,
sharedRoomId?}`), the human implicit as sender.

Per-agent capability configuration barely exists. The model is a **global**
setting (`SandStoredSettings.agentDefaultModel`,
`source/shared/node/settings/sand-settings-store.ts:21`) resolved at inference
time with experiment overrides layered on (`inference-service.ts:13`);
`AgentMetadata.lastUsedModel` records what was used, it does not configure. MCP
tool policy is likewise global, and the only per-agent persona is
`GroupMember.description` (`buildGroupMemberSystemPrompt`, `group-chat.ts:15`).

## The conversation unit, and how a transcript is addressed

The unit is **the agent**. One agent is one conversation is one directory is one
SQLite file (`store.db`, `session-paths.ts:9`); a group chat is itself an agent
directory with a `group.json` beside it. There is no separate session, thread or
channel entity — "channels" (`session/channel-store.ts`) are external connector
bindings, and threads are only a stamp on entries.

The transcript schema is three columns
(`source/host/extensions/session/agent-db-schema.ts:15`):

```sql
CREATE TABLE IF NOT EXISTS transcript_entries (
  seq INTEGER PRIMARY KEY,
  id TEXT NOT NULL UNIQUE,
  entry TEXT NOT NULL
) STRICT;
```

A message is addressed **both** ways: by monotone `seq` for paging and by a
stable string `id` for reference. That agrees with `tinyhivemind`, with one
twist — the string ids are *derived from transcript position*, not minted
randomly. `nextEntryId`
(`source/host/extensions/transcript/transcript-entry-ids.ts:5`) is a **pure fold
over the entries the caller already holds**: it counts user messages to get a
turn number and mints `t3u`, `t3a0`, `t3s1`, `t3ua0` — turn, kind, ordinal —
probing with `firstUnusedId` for the first free one; the boot turn is `"b"`. All
78 lines are pure and would port to Rust unchanged.

Paging is a fold over rows too. `readTranscriptPage` / `readTranscriptWindow` /
`readTranscriptTail` (`agent-db-transcript-pages.ts:14-16`) take a
prepared-statement bundle plus `{beforeSeq, sinceMs, untilMs, limit}`,
over-fetch by one to detect more, reverse the rows and return
`{entries, nextBeforeSeq?}`; the cursor is a `seq`. Visibility is pushed into
SQL as reusable fragments — `WINDOW_ENTRY_FILTER_SQL` drops tool-calls and
branched entries, `MAIN_TRANSCRIPT_MESSAGE_FILTER_SQL` keeps only sends,
attachments, user messages and messages carrying `fromAgent`/`toAgent`
(`agent-db-schema.ts:1,27`). That filter *is* the projection, written as a
`json_extract` predicate rather than a fold — exactly the piece `tinyhivemind`
keeps pure and portable.

## How a message causes a turn

There are two paths, and they follow different rules.

**One-to-one.** `SendPipeline.sendPrompt`
(`source/host/extensions/transcript/send-pipeline.ts:100`) dedupes on a
`clientNonce`: an in-flight nonce coalesces onto the running promise, an already
accepted nonce with a matching digest is a no-op, and a reused nonce with a
different digest raises `PromptAcceptanceDigestMismatchError`. The digest is a
SHA-256 over canonicalised `(agentId, prompt, richText, replyToId, isFork,
attachmentPaths, attachmentNames)`, and the ledger persists to
`send-acceptance.json`, capped at 256 records.
`dispatchUserTurn` mints one epoch and enqueues exactly one run; any in-flight
turn on that session is *interrupted*, not run alongside
(`send-turn-dispatch.ts:129`). This is one message, one turn.

**Group.** `send-group-fanout.ts:46` also enqueues exactly one task — but that
task is `runGroupTurn`, which builds a `GroupChatOrchestrator` and runs a
bounded sequential round-robin (`group-chat-orchestrator.ts:45`):

```ts
for (let round = 0; round < GROUP_MAX_ROUNDS; round += 1) {
  const responderIds = resolveResponders(members, this.deps.readHistory())...
  for (const memberId of orderRoundSpeakers(responderIds, round)) {
    if (totalMessages >= GROUP_MAX_MEMBER_TURNS || !this.deps.isCurrent()) return;
    const sent = await this.runOneTurn(args.group, member, members);
```

Each member turn is `await`ed — serial, never `Promise.all`. Bounds are
constants: `GROUP_MAX_ROUNDS = 3`, `GROUP_MAX_MEMBER_TURNS = 10`,
`GROUP_MAX_MESSAGES_PER_TURN = 2`, `GROUP_PROMPT_HISTORY_LIMIT = 24`
(`group-chat.ts:1`), and a round producing no messages ends the episode
(line 74). So the answer is neither fan-out nor one-turn: **one message, one
bounded sequential episode of up to ten turns**, cancellable by epoch.

### The mention grammar

It exists, it is host-side, and it applies only in group chats — there is no
mention parsing anywhere in the `send-*` pipeline.

- `memberMentionHandles(name)` (`group-chat.ts:7`) derives three handles from a
  display name — lowercased whole name, whitespace-stripped variant, first
  token. There is no stored `@handle` field anywhere.
- `hasMentionAt(lower, handle)` (line 8) scans for `@handle` with an ASCII
  `[a-z0-9]` word-boundary test.
- `parseGroupMentions(text, members)` (line 9) returns
  `{isEveryone, memberIds}`, `isEveryone` being
  `/(?:^|[^a-z0-9])@(everyone|all)\b/.test(lower)`.
- `resolveResponders(members, history)` (line 10) walks back to the last user
  message, unions mentions across every message since, and returns **all members
  if `@everyone`/`@all` appeared *or if nothing was mentioned***; otherwise the
  mentioned subset.

All four are pure functions of `(text, members)` or `(members, history)`. The
"unmentioned means everyone" default is the notable design choice, and it is
what turns a plain group message into a ten-turn episode.

Three more pure pieces sit beside them. `isPassContent` (line 11) recognises
`(pass)` — members are instructed to send exactly that when they have nothing to
add — and `isPotentialPassPrefix` suppresses the stream before the word
finishes; `orderRoundSpeakers` (line 3) rotates the opener by round; and
`messagesSinceMemberLastSpoke` (line 16) is the per-viewer projection, slicing
history from that member's last utterance to build its turn prompt.

### Scheduling

`SandRunScheduler` (`run-scheduler.ts`) keeps one queue per `agentId` with three
lanes — `pendingUser`, `pendingAgent`, `pendingBackground` — and one `active`
slot: strict single-flight per agent. `takeNextUserTask` (line 45) prefers a
non-`"group-member"` source within the user lane, so a direct message to a
member jumps ahead of that member's pending group slot; a generation counter
guards against a late settlement clobbering a newer run (line 259).

There is a watchdog: after `RUN_WATCHDOG_DEFAULT_MS = 120_000` with a user-lane
item waiting, the wedged run is interrupted; after a 30-second grace,
`escapeWedgedRun` force-resolves the stuck promise, parks it in a `zombies` set
and pumps the next task rather than deadlocking. `TurnRuntime.runTurn`
(`turn-runtime.ts:327`) short-circuits a stale epoch as `"superseded"`, and
`ensureUserReply` (line 534) re-invokes the runner up to `MAX_REPLY_NUDGES = 3`
times if the model owed a delivery and produced none.

### Agent-to-agent messaging

An agent addresses another by explicit id through a `SendToAgent` tool
(`source/host/agents/agent-messaging.ts:6`), not by parsing `@`. Guards are thin
(`agent-to-agent-messaging.ts:60`): empty message, self-message, deleted target,
remote-room target; text clamped at 8,000 characters. Delivery is asynchronous —
appended to `pendingAgentInbound` and drained by `reviveForAgentInbound`
(line 149) onto the recipient's `"agent"` lane.

**There is no hop bound, TTL or cycle detector between distinct agents.** A→B→A
ping-pong is prevented only by prompt text: "Respond only when you actually have
something to say or were asked something — if there is nothing to add, just
stop, so two agents never ping-pong acknowledgements"
(`agent-messaging.ts:76`). Contrast the group path, whose bounds *are*
code-enforced. `source/packages/agent/loop-detection/` does not help: it detects
*textual* self-repetition within one generation and aborts with
`AgentLoopError`; it is not a recursion guard.

## The coordinator/host split, and what counts as a port

`source/host/ports/` is not a hexagonal port layer. It is seven small files of
constants, error classes and helpers: `box.ts` (7 lines) holds strings and error
classes but declares no box interface (that is `CapableBox` in
`host/box/box-capabilities.ts`); `transport.ts` (5 lines) is a last-message-id
tracker; `telemetry.ts` describes a ~25-method surface only implicitly, through
the keys of `createNoopSandTelemetry()`. Two files do hold real injection points
(`SandMcpProvider`, `SandProductAnalytics`). Naming the folder `ports/`
overstates it.

The real seams are elsewhere, and they are good ones:

- `TurnExecutor` (`host/extensions/turn-execution/extension.ts:10`) — three
  methods bound late by the composition root, with an explicit double-bind
  error: "a second executor would mint a second runner for the same agent".
- `GroupOrchestratorDeps` (`group-chat-orchestrator.ts:17`) — six methods
  (`resolveMembers`, `readHistory`, `isCurrent`, `runMemberTurn`,
  `postMemberMessage`, `finalizeMemberTurn?`); the orchestrator does no IO. This
  is the repository's closest analogue to `tinyhivemind`'s hive episode: a
  bounded loop over a history the caller supplies, waiting pushed behind an
  interface.
- `HostRosterBookkeeping` (`host/host-roster-bookkeeping.ts:46`) — six named
  single-purpose interfaces resolved by an overloaded `api(id)`.
- `TranscriptPageStatements` (`agent-db-transcript-pages.ts:5`) — the paging
  functions depend on three prepared statements, not on a database.

Against that, `host/host-runner-composition.ts` is 2,646 lines and
`TranscriptManagerLike = Record<string, any>` (`transcript-hub.ts:43`) is how
most of the transcript extension refers to its own container. The discipline is
real in the leaves, absent in the trunk.

## The inference router

**One global setting, re-read per turn, with no signal from the agent or the
message.** `SAND_INFERENCE_PROVIDERS = ["cursor", "claude-code", "codex",
"openrouter"]` (`source/shared/inference-router.ts:1`); the choice is one
optional field `inferenceProvider` on `SandStoredSettings`
(`sand-settings-store.ts:29`), defaulting to `"cursor"`. Selection is a plain
branch:

```ts
// source/host/extensions/inference/inference-service.ts:56
createSession(onRequestId, sessionOptions) {
  const provider = routerSettings.getInferenceProvider();
  if (provider === "cursor") return routedSession(cursor.createSession(...), provider);
  return createProviderPromptSession(provider) as ...;
}
```

`getInferenceProvider()` reads and JSON-parses `settings.json` synchronously
each call (`sand-settings-store.ts:95`), freshly on every `createSession`, every
routed `sendPrompt` (`node-agent-coordinator/inference-router.ts:188`), and in
three other places. So it behaves per-turn, but the signal is always the same
user toggle: no cost-, latency-, capability- or task-based routing exists.

Within a provider the model is an env/config lookup, not a decision:
`configuredCodexModel()` reads `SAND_CODEX_MODEL` or `~/.codex/config.toml`
(`provider-session.ts:134`), Claude Code takes `SAND_CLAUDE_MODEL` (line 213),
OpenRouter takes `SAND_OPENROUTER_MODEL` (line 248). Only the `"cursor"` branch
reaches the original Statsig-experiment machinery
(`sand-model-experiment.ts`), and the router only decides whether to enter it.

## Approvals, permissions, tools, sandboxing

There are **two unrelated approval models**, and several nearby files that
sound like gates are not: the `prompt-acceptance-ledger` / `ack-obligations` /
`send-acceptance` cluster is delivery idempotency and crash recovery, and
`host/extensions/action-audit/` is a telemetry sink that never blocks anything
(`action-audit-service.ts:9`).

**Model A — local tool permission.** A globally persisted three-valued setting
(`SAND_LOCAL_TOOL_PERMISSIONS = ["always","ask","never"]`,
`source/shared/local-tool-permission.ts:1`) plus an ephemeral per-scope approval
cache. Per-request resolutions are four-valued — `"allow-once" | "deny" |
"always" | "never"` (`local-tool-permission-controller.ts:20`) — where
`always`/`never` writes back to the global setting and `allow-once`/`deny`
touches only the cache. The **scope key** is
`askKey(scope, request) = ${agentId}\0${toolCallId}\0${action}\0${target}`
(line 34): one tool call, one agent, one action, one target. `completeScope`
(line 48) retires every approval for that tool call unless marked
`outlivesScope`; an approval may instead be pinned to a `resourcePath`, which is
how a grant survives across calls touching the same file or terminal folder.

Whether a stored approval covers a new request *is* a pure predicate —
`localToolApprovalCovers` (`shared/local-tool-permission-machinery.ts:83`), four
lines: same action and target, or the same normalised resource path. It is
reused by the out-of-process local-exec daemon
(`host/local-exec/local-exec-daemon.ts:59`) — one decision function, two
processes. `authorize` itself (line 47) is not pure: it also consults remembered
refusals and an epoch. That epoch is the mechanism worth stealing: a refusal is
remembered against the *direction epoch* in which it happened (line 64), and
when the user gives new direction the epoch advances and stale refusals stop
applying. Widening the setting to `"always"` records `alwaysGrantedAtEpoch` per
agent so a call minted *before* the grant does not silently benefit from it
(lines 65, 67). Memory is bounded: 64 settled ids, 512 refusals per agent.

**Model B — auto-review.** `SandAutoReviewController`
(`source/host/runner/sand-auto-review.ts:108`) is a per-agent queue of pending
approvals, each with an `expiresAtMs`, a `userMessageEpoch` and a
`hostGeneration` fence. `requestApproval` returns a promise settled by
`resolveApproval` (line 138), expiry, cancellation, or supersession —
`beginUserMessageEpoch()` expires every pending approval when the user
redirects. Modes resolve per surface (`hostShell`, `boxShell`, `mcp`, `computer`,
`cloudAgent`, `subagentLaunch`) to `off`/`shadow`/`enforce` (line 12), and
*whether* to ask is decided by a classifier: a per-action, per-turn, model-gated
approval rather than a stored permission set, and the gate MCP calls pass
through in `enforce` mode.

**A third gate is attention, not permission.** `evaluateAutomationSpendGuard`
(`transcript/sand-automation-spend-guard.ts:32`) is a **pure function** from
`{nowMs, lastViewedAtMs, unreadCount, firesSinceViewedCount, nudgedAtMs,
snoozedUntilMs, optedOut}` to one of seven verdicts (`opted-out`, `user-active`,
`snoozed`, `pause`, `awaiting-ack`, `nudge`, `below-thresholds`) against fixed
thresholds: 3 days idle, 15 unread, 20 automation fires since last viewed, a
3-day pause delay, a 30-day snooze (lines 2-6). The impure half
(`automation-spend-guard-runtime.ts:82`) reads state, posts a card and disables
automations. Despite the name it measures runs-while-unread, not money — a
response-threshold rule with the decision already lifted out of IO.

### Tools and MCP

Tools are **not** registered one-per-server into the model's tool list. Two
meta-tools are minted per turn: a discovery tool `GetMcpTools`
(`packages/agent/tools/mcp/get-mcp-tools.ts`) and one generic dispatcher
`call_mcp_tool` (`packages/agent/tools/mcp/mcp.ts:438`) taking
`{server, tool, arguments}`. The routing table is `session.serverDescriptors`,
keyed by `serverIdentifier` (line 461), with `McpServerDoesNotExistError` on a
miss; the same grouping appears host-side in `createSandMcpStateExecutor`
(`host/ports/mcp-state-executor.ts:7`), bucketing `SandMcpTool` records by
`providerIdentifier`.

Execution never happens in-process: it goes through `mcpExecutorResource`
(`packages/agent-exec/mcp.ts:149`), a serialised RPC resource pointing at either
the host or the box daemon. For the added router, Claude Code gets a real local
HTTP MCP bridge (`node-agent-coordinator/routed-mcp-bridge.ts:39`) while Codex
and OpenRouter get a direct tool-calling loop (`provider-session.ts:153`, `230`)
proxying back to the same dispatch — two mechanisms, one inventory.

### Sandboxing

A "box" is a separate execution environment running a small Connect daemon
(`source/box-exec-daemon/server.ts:9`) exposing `ExecService`/`ControlService` —
shell spawn, read/write, ping, `UpdateEnvironmentVariables`, `LoadMcpServers`.
The isolation boundary is process/container plus an authenticated network hop
(`BoxEndpoint{host, port, authToken, headers}`), not a permission check. Three connectors implement the same `SandRemoteHostConnector` interface
(`electron-main/box/box-host-connector.ts:38`) and are interchangeable to the
host: loopback (host inside the same container, port 1337), brokered/remote
(`GrokBotService.ensureSandBox` returns a gateway URL and token, backoff on
`SAND_BOX_BLOCKED`), and local Docker (`local-docker-host-connector.ts`) —
`docker run` of a pinned image as `grok-bot-local-vm`, gateway on
`127.0.0.1:1340`, a locally generated bearer token, read-only credential mounts,
health-checked before the coordinator connects.

A *second*, unrelated sandbox covers commands the host runs on the user's own
machine: `spawnInSandbox` (`packages/shell-exec/sandbox/sandbox.ts:10`) wraps
the spawn in a macOS Seatbelt profile with filesystem allow/deny lists and a
network deny list, unless the policy is `"insecure_none"`.
`assertPathOutsideProtectedRoots` (`host/box/protected-path-guard.ts`) tests a
candidate both as resolved and as `realpath`, so a symlink cannot escape. Agent
state is isolated further in worker processes (`host/agent-isolation/`), each
agent owning its own `conversation-blobs.db`.

## Usage and cost tracking

Measured per provider: `requests`, `inputTokens`, `outputTokens`,
`cacheReadTokens`, `cacheWriteTokens`, `lastUsedAt`
(`source/shared/inference-router.ts:4`). No cost figure is retained. The
accumulation *is* a fold — `recordInferenceUsage` (`sand-settings-store.ts:161`)
computes `previous + delta` over the record it already holds, clamping each
delta through `Number.isFinite(v) && v >= 0 ? Math.round(v) : 0`. It is wrapped
in a full read-JSON/mutate/write-JSON round trip per turn, so the fold is right
and the storage is not. Claude Code returns a real `total_cost_usd`, captured
into per-turn provider metadata (`provider-session.ts:223`) and then dropped —
never accumulated, never displayed.

## What is already a fold over data the caller holds

Ordered by how directly it maps onto `tinyhivemind`:

1. The whole mention grammar, pure over `(members, history)` —
   `resolveResponders`, `parseGroupMentions`, `memberMentionHandles`,
   `hasMentionAt` (`host/groups/group-chat.ts:7-10`) — together with
   `orderRoundSpeakers` (line 3), `isPassContent` / `isPotentialPassPrefix`
   (line 11), `messagesSinceMemberLastSpoke` (line 16) and `formatGroupHistory`
   (line 14): speaker rotation, response threshold, per-viewer projection.
2. `GroupChatOrchestrator.run` (`group-chat-orchestrator.ts:35`) — a bounded
   episode loop, pure but for six injected methods.
3. `evaluateAutomationSpendGuard` (`sand-automation-spend-guard.ts:32`) — a
   seven-verdict decision over a window.
4. `nextEntryId` and its counting helpers (`transcript-entry-ids.ts`) — the
   whole 78-line file.
5. `localToolApprovalCovers` (`shared/local-tool-permission-machinery.ts:83`) —
   a four-line permission predicate shared by two processes.
6. `truncatePromptFairly` / `computeMaxMinFairAllocations`
   (`packages/agent-summarization/prompt-truncation.ts:17,70`) — **max-min fair
   allocation** of a character budget across messages: sort by size, hand each
   the smaller of its size and `floor(remaining/remainingCount)`, drop anything
   under `minUsefulChars = 200` and mark it `[omitted N chars]`. Directly
   reusable as an attention-market fold.
7. `resolveHostExtensionBootOrder` (`internal/host-extensions.ts:57`), a
   topological sort with named errors, and `parseCoordinatorFrame`
   (`shared/rpc/coordinator-port.ts:35`), a non-throwing frame parser.
8. Smaller ones: `upsertAgentSummary` (`shared/agents/agent-summaries.ts:13`),
   `getLastEntryFromTranscript` (`session/session-projection.ts:14`), the paging
   folds in `agent-db-transcript-pages.ts`, `estimateTokenCount`
   (`agent-summarization/token-estimate.ts:67`), and
   `single-message-loop-detector.ts` (config injected as thunks).

## Where the README overclaims

Very little, and the drift is about *layering* rather than *existence*: all four
added features are real, IO-performing implementations, not stubs.

- **"an inference router"** — the word oversells it. It is a four-valued enum in
  a settings file with a `switch`: no routing policy, no fallback, no per-agent
  or per-task selection. Read as "a provider switch", the claim is exact.
- **"Grok Bot plugin/MCP tools across the routed providers"** — true, but via
  two unlike paths (an HTTP MCP bridge for Claude Code, a direct tool loop for
  Codex and OpenRouter). The phrasing implies one mechanism.
- **"local usage tracking"** — accurate, and more careful than it needed to be:
  "request and token totals", with billing authority disclaimed.
- **"an optional local Docker sandbox"** — accurate line for line.
- The genuine overclaim is architectural, and the repository makes it in its own
  vocabulary rather than in the README: `source/host/ports/` is not a port
  layer, and `RosterProjection` is not a projection. The README diagram renders
  `coordinator + host` as one box, hiding that the host is 35 extensions and a
  2,646-line composition root.
- `frontend/` is honestly described as a "readable partial reconstruction".

## Mechanism → what `tinyhivemind` already has → what it does not

| Mechanism (as found) | `tinyhivemind` already has | Not covered here |
| --- | --- | --- |
| Mention grammar `@name` with three derived handles per member (`group-chat.ts:7`) | `docs/specs/mentions.md` — roster records, grammar, normalization | Deriving handles from a display name rather than storing one; the ASCII-only word boundary |
| `@everyone` / `@all` regex → all members (`group-chat.ts:9`) | `@everyone` as a list, one message one turn | Their default: **no mention also means everyone** |
| `resolveResponders` over history since last user message | Responder ladder, `docs/specs/responders.md` | Unioning mentions across *every* message since the last user turn, not just the triggering one |
| Bounded episode: 3 rounds × 10 member turns × 2 messages, ends on a silent round (`group-chat-orchestrator.ts:45`) | the `hive` module's episode state machine, sequential by ADR 0002 (since superseded by ADR 0014) | Their caps are plain constants, not a policy record; the "silent round ends it" stop rule |
| `"(pass)"` convention + `isPotentialPassPrefix` streaming suppression | Response thresholds, cross-inhibition | An explicit *textual* pass token the model emits, and prefix-suppression while it streams |
| `orderRoundSpeakers` rotation; `messagesSinceMemberLastSpoke` + `formatGroupHistory(limit=24)` | One-responder selection; projection fold, `docs/specs/sessions.md` | Rotating the opener by round number; slicing from the viewer's own last utterance as the whole prompt input |
| `seq` + stable `id` on one table, ids position-derived by `nextEntryId` (`agent-db-schema.ts:15`, `transcript-entry-ids.ts:5`) | Sequence-number addressing across host surfaces | Nothing on `seq` — that agrees. New: deriving the string id from a fold over the transcript, so it is reproducible |
| SQL visibility filters as the projection (`agent-db-schema.ts:1,27`) | Pure projection fold | Pushing the filter into the store — the opposite trade; worth knowing as a host-side option |
| `clientNonce` + canonical SHA-256 digest ledger, 256 records | Idempotent dispatch, `docs/specs/mention-dispatch.md` | The digest-mismatch error on a reused nonce; a bounded persisted ledger |
| Epoch supersession + `MAX_REPLY_NUDGES = 3` (`turn-runtime.ts:534`) | Bounded turns | Re-invoking a runner that owed a delivery and produced none |
| Three-lane per-agent queue with watchdog and zombie escape (`run-scheduler.ts`) | `MentionTurnQueue` port, atomic enqueue | The wedged-run escape hatch and lane preemption of `group-member` by `user` |
| Agent-to-agent send with **no** hop bound (`agent-to-agent-messaging.ts:60`) | Hop-bounded dispatch, `cross-desk-referral.md` | Nothing to take — this is the failure mode `tinyhivemind` exists to avoid, observed in the wild |
| `localToolApprovalCovers` pure predicate + `agentId\0toolCallId\0action\0target` scope; direction epochs invalidating refusals and standing grants (`local-tool-permission-controller.ts:34,64-67`) | — | A whole approval algebra: scope key, `outlivesScope`, resource-path pinning, one predicate reused by two processes, and epoch-scoped consent so a grant cannot retroactively cover an older request |
| `evaluateAutomationSpendGuard` seven-verdict window fold | Decaying salience, quorum | A pure "should I keep spending attention here" verdict over unread/idle counters |
| `truncatePromptFairly` max-min fair char allocation (`prompt-truncation.ts:17`) | Attention market, stated per-message budget | The concrete max-min algorithm and the `minUsefulChars` drop-and-mark rule |
| Provider switch as a global enum re-read per turn; usage fold `previous + clamped delta` (`inference-service.ts:56`, `sand-settings-store.ts:161`) | Model selector boundary, `docs/specs/responders.md` | Nothing from the switch — it confirms the selector belongs behind a port. New: a per-provider counter fold (their JSON-round-trip storage is the part not to copy) |
| `RemoteRoomMember {kind: "agent" \| "human", authId, agentId}` | Roster records distinguishing participants | Carrying an owning account id alongside the agent id for cross-user rooms |
| One agent = one conversation = one directory = one SQLite file | Host owns storage; desks as overlays | Their conflation of identity and conversation — `tinyhivemind`'s desk overlay is the thing they lack |
