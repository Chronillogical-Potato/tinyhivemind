# lab

A standalone Cargo workspace for the hive lab: trace sinks, runnable examples
and the SWE hive. It stays outside the library workspace so core keeps its
pure dependency tree. The root README of the repository describes the library.

| Path | Purpose |
| --- | --- |
| `src/` | shared plumbing (`JsonlSink`, `WallClock`, in-memory log), `bin/` examples, and `swe/`, the SWE hive and baseline |
| `viewer/` | offline viewer for the JSONL traces |
| `harbor/` | Harbor agent wrapper for Terminal-Bench |
| `tests/` | mock model server and the offline end-to-end script |
| `docker/cortex/` | a local CortexDB server for `swe_hive --memory cortex` |

Contract checks: `cargo fmt --all -- --check`, `cargo clippy --all-targets -- -D warnings`, `cargo test`, run from this directory.

## Knob examples

Six offline, deterministic binaries drive every public knob of
`tinyhivemind-core`: no network, no model, no clock, no randomness. The same
command prints the same text twice. A scripted `Digester`, a keyword `Router`,
a scripted System One transport and rule-based seats stand in for models, and
`MemoryLog` stands in for the host's journal. What each prints is a fold over
those, so a changed line is a changed behaviour.

| Binary | Drives | Run |
| --- | --- | --- |
| [`memory_hive`](src/bin/memory_hive/README.md) | digest (plan, fold, accept, apply), pins, continuous sharing, team briefing, elsewhere, thread index, the `SessionLog` contract | `cargo run --bin memory_hive` |
| [`context_tools`](src/bin/context_tools/README.md) | the room's tool surface (`tool_specs`, `interpret`, `commit_utterance`), asides, projection and redaction, masking, mentions | `cargo run --bin context_tools` |
| [`swarm_knobs`](src/bin/swarm_knobs/README.md) | `EpisodePolicy` swept one knob at a time over `hive::step`, bids, salience, directory, division, exchange, evaluated quorum, trace grammar, telemetry sinks | `cargo run --bin swarm_knobs` |
| [`driver_knobs`](src/bin/driver_knobs/README.md) | `CompletionDriver` (width, queue depth, broadcast budget), `Conductor` (walls, parking, snapshot and resume), routing policy, `JevRouter`, the completion fold, hive validation, the brief | `cargo run --bin driver_knobs` |
| [`relay_hive`](src/bin/relay_hive/README.md) | a bug handed across three desks by referral, swept over `ReferralPolicy`, and the full refusal gallery | `cargo run --bin relay_hive` |
| [`gate_knobs`](src/bin/gate_knobs/README.md) | chat identity, the desk overlay and roster states, `mention_dispatch`, `responder_plan`, `approve` | `cargo run --bin gate_knobs` |

`memory_hive`, `context_tools`, `swarm_knobs`, `driver_knobs` and `relay_hive`
accept `--trace out.jsonl` and write the telemetry the [viewer](viewer/README.md)
reads. Timestamps come from `TickClock`, one millisecond per event, so a trace is
as repeatable as the text. `gate_knobs` is a table of pure verdicts and emits no
trace.

```sh
cd examples/lab
cargo run --bin swarm_knobs -- --trace /tmp/swarm.jsonl
cargo clippy --all-targets -- -D warnings && cargo fmt --check
```

Findings from building these are kept in
[`docs/experiments/2026-10-04-findings.md`](../../docs/experiments/2026-10-04-findings.md).
Row numbers in the matrix below (`F12`) point into its knob-lab tables.

## Coverage matrix

Every public policy, constant and entry point of core, and the example that
drives it. A cell names the section of the binary's output. `printed` means the
value is shown but no behaviour was swept; the last section lists what no
example could drive and why.

### Runtime: memory

| Knob | Example | Where |
| --- | --- | --- |
| `DigestPolicy.keep_live`, `fold_after`, `input_limit`, `budget_chars` | `memory_hive` | digest: plan table, refold to a fixed point |
| `DigestPolicy.fold_after_chars`, `from_token_budget`, `CHARS_PER_TOKEN` | `memory_hive` | digest: size trigger at head ^16 |
| `ChannelHead.sequence`, `unfolded_chars`, `ChannelHead::at` | `memory_hive` | digest |
| `plan_digest`, `collect_digest_input`, `accept_digest`, `refold`, `apply_digest` | `memory_hive` | digest (all five; `DigestGap`, `DigestConversationChanged`) |
| `Digester`, `DigestRequest.prior`, `.messages`, `.through`, `.budget_chars`, `.pinned` | `memory_hive` | scripted digester |
| `DigestOutcome` (`Current`, `Folded`, `Unavailable`, `Rejected`), `DigestRejection` (`Empty`, `TooLarge`, `Regressed`) | `memory_hive` | digest: every way a fold fails |
| `PIN_LIMIT`, `PIN_EXCERPT_CHARS`, `PIN_MARKER_CAP`, `fold_pins(limit)`, `read_pinboard(limit, before)`, `read_directives`, `pin_note` | `memory_hive` | pins |
| `PIN_SCAN` | `memory_hive` | printed |
| `PRESENT_SET_LIMIT`, `note_present`, `prepare_delta`, `initialized_state`, `SharingQuery.before` | `memory_hive` | sharing |
| `ReinitializeReason` (`ConversationChanged`, `GapTooLarge`, `WatermarkUnavailable`), `WatermarkRegression`, `PresentSetOverflow`, `PresentSetTooLarge` | `memory_hive` | sharing |
| `SESSION_WINDOW`, `SessionQuery.window`, `.before`, `.viewer`, `.conversation` | `memory_hive`, `context_tools` | digest window; projection |
| `SCAN_LIMIT`, `PAGE_SIZE` | `memory_hive` | digest gap and sharing gap over 2,100 rows |
| Page validation errors (`PageTooLarge`, `EmptyPageCursor`, `DuplicateSequence`, `PageNotDescending`, `CursorAfterOldest`, `PageOutOfRange`, `CursorDidNotAdvance`, `Read`) | `memory_hive` | SessionLog contract |
| `TeamBriefing` (`from_snapshots`, `teammates`, `BriefedTeammate.role`, `.description`), `system_text`, `system_text_with_dispatch` | `memory_hive` | briefing |
| `BrevityPolicy.message_chars`, `.window`, `overrun`, `rule_text` | `memory_hive` | briefing |
| `MentionDispatchContext`, `MentionDispatchPolicy.enabled`, `.max_hops` | `memory_hive`, `gate_knobs` | briefing; dispatch gallery |
| `initialize_session`, `initialize_session_with_context`, `SessionContext`, `BriefingNote` | `memory_hive` | briefing |
| `ElsewhereQuery.seat`, `.conversations`, `.current`, `.before`, `.window`, `gather_elsewhere`, `render_row` | `memory_hive` | elsewhere (F9, fixed: General aliases skipped) |
| `THREAD_INDEX_LIMIT`, `THREAD_OPENING_CHARS`, `read_thread_index`, `ThreadLine.landed` | `memory_hive` | threads |
| `THREAD_INDEX_SCAN`, `fold_thread_index` (direct) | `memory_hive` | printed; folded through `read_thread_index` |

### Runtime: speech, asides, masking, mentions

| Knob | Example | Where |
| --- | --- | --- |
| `tool_specs`, `ToolSpec`, `ToolParameter`, `ParameterKind`, `READ_DEFAULT`, `READ_MAX`, `read_limit` | `context_tools` | tool_specs table |
| `interpret` for `post`, `broadcast`, `dm`, `ask`, `ask_teammates`, `complete_episode`, `close`, `read` | `context_tools` | interpret |
| `UtteranceRejection` (`UnknownTool`, `EmptyText`, `NoRecipients`, `UnknownRecipient`, `SelfRecipient`, `OneRecipient`, `NotAGroup`) | `context_tools` | interpret; check_recipients |
| `Utterance` helpers, serde alias `close` | `context_tools` | check_recipients and addressed_peers |
| `commit_utterance`, `CommitRequest.aside`, `CommittedUtterance.refusal`, `commit_utterance_to_room`, `Error::AsideRefused` | `context_tools` | commit_utterance under three policies; a declined aside is refused, the room fallback is opt-in (F13, fixed) |
| `CommitRequest.spent`, `.unsettled`, `AsideInput.spent`, `.unsettled` | `context_tools` | swept through `aside()`; `commit_utterance` is called with `0` and `false` |
| `addressed_peers`, `check_recipients` | `context_tools` | check_recipients and addressed_peers |
| `fence::extract_post` | `context_tools` | fence |
| `AsidePolicy.enabled`, `.max_members`, `.max_messages`, `.must_surface`, `.require_thread` | `context_tools`, `memory_hive` | aside(); briefing |
| `NoAsideReason` (all eleven), `AsideInput`, `Audience`, `Viewer` | `context_tools` | aside(); projection |
| `project_session`, `project_as`, `Elision.settled_at` | `context_tools` | projection |
| `code_ranges`, `fenced_ranges`, `is_masked` | `context_tools` | masking |
| `resolve`, `direct_responder`, `mentioned_members`, `MENTION_CAP`, `MentionAuthor` | `context_tools` | mentions |
| `mention_dispatch`, `NoDispatchReason` | `gate_knobs`, `context_tools` | dispatch gallery |

### Hive

| Knob | Example | Where |
| --- | --- | --- |
| `EpisodePolicy.turn_budget`, `round_width`, `revealed_width`, `blind_round`, `dominance_cap`, `repetition_cap`, `distance`, `quorum`, `weights` | `swarm_knobs` | sweep, one row per setting |
| `EpisodePolicy::for_room`, `DEFAULT_ROUND_WIDTH`, `DEFAULT_REVEALED_WIDTH` | `swarm_knobs` | sweep `for_room` |
| `QuorumPolicy.threshold`, `.window`, `.require_grounded` (F17) | `swarm_knobs` | sweep `quorum.*` |
| `AdmissionPolicy.maximum_violation_probability`, `DecisionEvaluation`, `step_with_evaluations`, `standings_with_evaluations` (F20) | `swarm_knobs` | quorum by evaluation |
| `SalienceWeights.recency`, `.importance`, `.relevance`, `.half_life`, `for_room`, `salience`, `importance` | `swarm_knobs` | sweep `weights`; salience table |
| `AgentThreshold.threshold`, `.affinity`, `bids`, `floor_holder`, `floor_round` | `swarm_knobs` | sweep `thresholds`; bids |
| `Basis`, `Horizon::at`, `::over`, `distance`, `within` (F15) | `swarm_knobs` | sweep `distance`; salience table |
| `DirectoryPolicy.half_life`, `.specialisation`, `.credibility`, `.prior`, `.discredit`, `.window`, `.floor`, `directory`, `Directory` accessors | `swarm_knobs` | directory + division |
| `DivisionPolicy.round_width`, `.follow_directory`, `divide`, `Division` accessors | `swarm_knobs` | directory + division |
| `ExchangePolicy.enabled`, `.contact_cap`, `.round_cap`, `ExchangeState`, `NoExchangeReason` | `swarm_knobs` | exchange |
| `resolve` (traces), `TRACE_CAP`, `TraceKind`, `Trace.grounded` | `swarm_knobs` | trace grammar |
| `HiveStep` (`Speak`, `Converged`, `Deadlocked`, `Exhausted`, `Idle`), `project_for`, `Phase`, `Visibility` | `swarm_knobs` | sweep |
| Completion fold (`CompletionEpisodeState`, `apply_assignment`, `apply_completion`, `completion_status`) | `driver_knobs` | hive::completion |
| Hive errors (`ZeroRoundWidth`, `ZeroQuorumThreshold`, `ZeroQuorumWindow`, `ZeroHalfLife`, `ZeroDirectory*`, `UnknownThresholdMember`, `DuplicateAgentThreshold`, `NoSeats`, decision-evaluation errors, completion errors) | `swarm_knobs`, `driver_knobs` | refused policies; completion fold |

### Driver, embed, typesafe, referral

| Knob | Example | Where |
| --- | --- | --- |
| `CompletionDriver::new(round_width)`, `with_queue_depth`, `with_broadcast_budget` | `driver_knobs` | construction; broadcast; Conductor sweep |
| `BroadcastRouting.primary`, `.policy`, `.roster_version` | `driver_knobs` | broadcast; Conductor |
| `BroadcastRouting.reasoning`, `.thread_context` | `driver_knobs` | `reasoning` driven through `route_message` only; see gaps |
| `DriverState` (`episode`, `revision`, `ledger`, `seen`, `delivered`, `turn_started`, `quiescent`, `stalled`), `Ledger`, `Handoff`, `Seen` | `driver_knobs` | broadcast; replay; ordering |
| `pending_round`, `apply_committed`, `Transition`, `HostAction` | `driver_knobs` | pending rounds; broadcast |
| `ConductPolicy.child_turn_wall`, `.turn_wall` | `driver_knobs` | Conductor sweep (F28) |
| `Door`, `starters`, `Conductor::open`, `begin_wave`, `turns`, `open_turn`, `record`, `step`, `committed` | `driver_knobs` | Conductor |
| `Conductor::snapshot`, `resume`, `ConductorState` (F27), `record_parked`, `resume_seat`, `parked`, `conversations_involving`, `shown_conversations` | `driver_knobs` | replay table; parked row |
| Every conductor `Event` | `driver_knobs` | Conductor sweep (nudged, parked, resumed, broadcast, unplaced, completed_by_broadcast, asked, handoff, refused, discharged, concluded) |
| `BoundHive`, `HiveGraph`, `AgentBinding`, `resolve_dm`, `resolve_plan`, `desk_request`, `route_desk` | `driver_knobs` | BoundHive gallery; routing |
| `EpisodeBrief`, `Channel`, `ConversationView`, `ElsewhereView`, `standing_contract`, `speaker` | `driver_knobs` | EpisodeBrief |
| `RoutingPolicy.minimum_confidence`, `.high_impact_minimum_confidence`, `.clarification_threshold`, `.high_impact_threshold`, `.round_width`, `.choice_option_limit` (F24, F25) | `driver_knobs` | embed routing table |
| `route_message`, `route_broadcast`, `RoutingFallback` (all nine), `ConversationKind`, `CONCURRENT_CHOICE_THRESHOLD_PARTS` | `driver_knobs` | routing; bypasses |
| `JevRouter::new`, `with_model`, `SystemOneTransport`, `classify_retry`, typesafe errors (F26) | `driver_knobs` | JevRouter table |
| `ReferralPolicy.enabled`, `.max_hops`, `.reach`, `.returns`, `ReferralReach`, `NoReferralReason` (F31-F33) | `relay_hive` | sweep; gallery |
| `ReferralInput.origin`, `ReferralOrigin`, `ReferralKind` | `relay_hive` | relay |
| `Tracer::emit`, `step`, `conducted`, every `TraceEvent` variant, `MemorySink`, `ManualClock`, `NullSink` | `swarm_knobs`, `driver_knobs`, `context_tools`, `memory_hive` | traces; telemetry sinks |

### Identity and gates

| Knob | Example | Where |
| --- | --- | --- |
| `is_general_chat`, `same_conversation`, `MAIN_THREAD_ID`, `GENERAL_DESK` | `gate_knobs` | chat identity |
| `DeskSet` (declared, added, additions, orders, retired, tombstoned), `lead`, `members`, `resolve_id`, `ResponderMode` | `gate_knobs` | desk overlay |
| Every desk and roster validation error | `gate_knobs` | invalid snapshots; roster |
| `Roster` states (active, retired, tombstoned), `Person` | `gate_knobs` | roster |
| `responder_plan`, `ResponderRequest.selection_policy`, `.minimum_selection_confidence`, `SelectorCandidate`, `accept_selection`, `accept_evaluation` (F35) | `gate_knobs` | responder |
| `ApprovalPolicy.enabled`, `.default`, `.rules`, `.approver`, `.allow_grants`, `.max_grant_ttl` | `gate_knobs` | approve |
| `ApprovalRule`, `TargetPattern`, `ApproverRule`, `DeskApprover`, `GrantScope`, `StandingGrant`, `RememberedRefusal`, `ScopeKey`, `Millis`, `ConsentEpoch` (F36) | `gate_knobs` | approve |
| Every `DenyReason`, `AllowBasis` | `gate_knobs` | approve |

### SWE hive: sessions and memory

`swe_hive` flags added for issue #104 (seats forgot what they ran), and what
exercises them. Unit tests run under `cargo test`; `offline.sh` is
`tests/offline.sh` against the mock model.

| Flag | Default | Where |
| --- | --- | --- |
| `--seat-session persistent` | yes | `seat/test/session.rs` (second activation keeps the first's tool results, delta-only resume, nothing removed without compaction), `hive/test.rs` (a woken lead resumes its own session; its own broadcast is not echoed), `board/test.rs` (delta watermark, own posts skipped, pins only when changed), `offline.sh` `hive-persistent` |
| `--seat-session fresh` | | `seat/test/session.rs`, `hive/test.rs` (`fresh_sessions_reproduce_the_briefing_per_activation`), `offline.sh` `hive-fresh` |
| `--hive-context mask|summarize` | `summarize` (mask, then summarize if still over budget); `mask` under `fresh` | `config/test.rs`, `context/test.rs` (`scaled_estimate`), `seat/test/session.rs` (compaction is what shrinks a session) |
| `--memory none|cortex`, `--memory-url`, `--memory-budget`, `--run-id` | `none`, `$CORTEX_DB_URL`, 1200, generated | `config/test.rs`; `memory/test.rs` on the reference engine (a later recall surfaces an earlier failed attempt, rejoin shows a teammate's memory once, two run ids share nothing, namespace root pinned, timeout and unreachable server degrade to no memory); `seat/test/memory.rs` (recall at start, rejoin, compaction; ledger stored at the end; a failing memory is reported and ignored); `live_cortex_memory_round_trip` and `offline.sh` `*-mem` with `CORTEX_DB_URL` set |
| `session` and `memory` marks | | `seat/test/session.rs`, `seat/test/memory.rs`, `offline.sh`, and their own timeline lanes in the viewer (`viewer/src/marks.js`) |

### Gaps: what no example drives

| Item | Why |
| --- | --- |
| Attention constants (`SPEAK_COST` 500, addressed 2000, dissent 1500, quiet 1000, dominance penalty 3000), directory deposit constants, `WEIGHT_CEILING` | Constants, not policy fields; nothing to sweep (F21). Their effect is visible only through the `bids` and directory tables. |
| `PIN_SCAN`, `THREAD_INDEX_SCAN` behaviour | Bite only past 2,048 and 256 rows; printed, not swept. `SCAN_LIMIT` is swept. |
| `NoDispatchReason::HopOverflow`, `NoReferralReason::HopOverflow` | Unreachable: `hop >= max_hops` trips first (F33). |
| `Error::InvalidProbability`, typesafe `Error::SerializeState` | Unreachable from outside: `Probability::new` validates; the state is always serializable. |
| `NoDispatchReason::SelfMention`, `TargetInactive`, `NoReferralReason::SelfMention`, `TargetInactive`, `UnknownDesk` from authored text | `resolve` removes them first; driven with hand-built mentions only (F33). |
| `BroadcastRouting.reasoning` and non-empty `thread_context` inside the driver | Every driver run passes `reasoning: None` and no context; the reasoning path is driven through `route_message` and `hive.route_desk`. |
| `MessageRoute::CurrentConversation`, `DirectAgent`, `DeskReferral` | Vocabulary the host acts on; nothing in core constructs them. |
| `SelectionDisposition::Selected`, `InvalidOutput`, `ResponderRung::AutoSelection` | Never produced by core; the host assembles them (F35). |
| `EpisodeBrief.elsewhere` filled by the conductor | By design the episode fills nothing in; the lab sets it by hand. |
| Serde wire forms of most payload types | Core pins them in its own unit tests; the lab round-trips only `DriverState`, `ConductorState`, `SharingState` and `Utterance`. |
| A real model behind `Digester`, `Router`, `SystemOneTransport` or a seat | Out of scope: the lab is offline. The scripted stand-ins have the same signatures. |
| `tinyhivemind-tools` | Not a dependency of the lab; `context_tools` uses core's `runtime::speech` instead. |
