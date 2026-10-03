# Attributed session projection and initialization

**Status:** Implemented  
**Owner:** tinyhivemind maintainers

## Problem

Agents sharing a conversation must read a bounded, chronological transcript
without losing who authored each message. The host owns the durable log, so the
collaboration runtime needs a narrow read port and a deterministic paging walk
instead of another store. A newly initialized agent also needs an ephemeral
description of its teammates and the rules of the shared room.

## Goals

- Read a host-owned log through an object-safe, runtime-neutral port.
- Project a bounded channel or thread history while preserving attribution.
- Reject malformed pagination before it can duplicate, skip, or loop history.
- Return a separate deterministic team briefing for session initialization.
- Construct a conservative briefing from host-supplied desk and roster
  snapshots without introducing host role types.

## Non-goals

- Storage, model clients, HTTP, runtime handles, watermarks, responder
  selection, or mention dispatch.
- Persisting the team briefing or assigning it a sequence number.
- Interpreting a peer message as the viewer's own prior reply.

## Proposed behavior

The `runtime` module lives in `tinyhivemind-core` alongside the pure algebra.
`Sequence(u64)` identifies host log rows. `Conversation` names a desk by its
canonical id and display name and optionally names a thread root. `LogMessage`
contains its sequence, optional stored chat id, optional direct parent,
`SessionAuthor`, and untouched content. `SessionPage` is newest-first and may
provide `next_before`, an exclusive cursor for an older page. That cursor may
equal or be older than the page's oldest row, but cannot be newer.

`SessionLog::read_before` returns a boxed, sendable future and a boxed,
sendable source error. It is object-safe and does not select an async runtime.
`project_session` reads at most `SCAN_LIMIT` (2048) raw rows in pages of at most
`PAGE_SIZE` (512), then returns at most the requested window (normally
`SESSION_WINDOW`, 30) in chronological order. A zero window performs no read.
The query's `before` cursor is exclusive, so the current message can be
excluded by using its sequence.

Each page is validated before use. Rows must be strictly descending, below the
requested cursor, and unique across the walk. A nonempty page's next cursor
must be no newer than its oldest row; an empty page cannot advertise another
cursor. Relative to the requested cursor it must strictly move toward older
rows. Violations are typed errors. Source read errors retain their source.
Reaching the scan cap is successful and returns the qualifying history found
so far.

Conversation filtering uses `tinyhivemind_core::chat::same_conversation` against
both the desk id and display name. Channel projection keeps only rows without a
parent. Thread projection keeps the root and its direct children, and stops
scanning once the root row is reached, even if that row has blank content.
Content whose trimmed form is empty is skipped; all other content bytes and
every author are preserved unchanged.

`TeamBriefing` identifies the viewer and desk and lists `BriefedTeammate`
records. A teammate has an id, label, and optional role and description.
`from_snapshots` validates host-supplied desk and roster snapshots. General
uses the active roster; another desk uses its effective active desk order.
It excludes the viewer, retired and unknown ids, and duplicates. Because the
core snapshots carry no agent role or description, these optional fields are
unset; hosts may construct richer validated records directly.

`TeamBriefing::system_text` deterministically identifies the viewer and desk,
lists teammate `@id` handles with optional metadata, and states that peer
messages remain attributed and are not the viewer's replies. Everyone, desk,
and person mentions are context only and never fan out.
`initialize_session` returns the briefing and projected history as separate
values; the briefing is never stored, sequenced, or counted against the history
window.

### What the briefing may offer

The mention-dispatch rule — that a direct agent mention may start at most one
bounded child turn — is stated only to a run that could actually use it. A
briefing carries a team, not a run: it has no hop and no policy, so
`system_text` cannot know, and withholds. `system_text_with_dispatch` takes a
`MentionDispatchContext`, which is the `MentionDispatchPolicy` and the hop the
host already passes to `mention_dispatch`, and states the rule when and only
when `MentionDispatchContext::may_dispatch` holds — `policy.enabled` and
`hop < policy.max_hops`, the same two guards `mention_dispatch` tests before it
reads a mention. A disabled policy, a zero hop budget, a hop at or past
`max_hops`, and a caller that supplies no context all render identical text:
the one without the offer. Nothing else in the briefing changes, and the
offered sentence is unchanged from the one that was previously unconditional.

Absent information therefore fails closed. This is the withhold rule of
[`responders.md`](responders.md) applied to the one surface a model reads:
offering a capability whose next call would refuse it teaches the model the
capability exists and spends a turn discovering that it does not. The context
is plain data passed at render time rather than a field on `TeamBriefing`, so
the briefing's wire form is unchanged and a host that never supplies one keeps
compiling — it just stops advertising dispatch.

### Why there is no transcript-repair fold

A stored transcript is not automatically a valid prompt. CopilotKit's OpenBot
learned this in production: an interrupted turn left a message referencing a
tool call whose result never landed, leaving the pairing the provider requires
dangling, and because the log is append-only *every later turn in that thread* failed —
permanent damage grown out of a transient fault. Its answer,
`sanitizeSeededHistory`, is a read-side repair fold: drop the dangling halves,
never rewrite an id, return an unchanged message identically. See
[`../research/grok-bots/copilotkit-openbot.md`](../research/grok-bots/copilotkit-openbot.md).

**Our shape cannot sustain that damage, and this specification deliberately
adds no repair step for it.** A `SessionMessage` is a sequence, an author, and
untouched content. Nothing in it names another message, so there is no pairing
for a projection to cut in half and no reference that can dangle: every
projected message is independently a valid prompt entry, whatever else was
dropped around it. Inventing a damage model to repair would mean inventing the
tool-call structure we do not carry.

The two references the runtime *does* carry are already resolved on the read
path, which is the same insight arriving in a smaller form:

- `LogMessage::parent` is structural, and it is the one thing that could
  dangle. `narrow_to_roots_and_first_replies` drops a reply whose root fell
  outside the scan rather than flattening it into the channel, so an answer is
  never presented as a statement whose question the reader never saw
  (`channel_projection_drops_a_reply_whose_root_is_outside_the_scan`).
- `!pin ^N` and a hive `^cite` name a sequence *inside content*. Both resolve
  best-effort — a pin outside the scan keeps its sequence and reports no
  excerpt — and neither can invalidate the message carrying it.

Repair is therefore already fused into the projection rather than bolted beside
it, and the properties OpenBot's fold had to be careful to preserve are ours by
construction: sequences and attribution are never rewritten, content bytes are
returned unchanged, and a message needing nothing is returned identically
(`skips_trim_empty_content_but_preserves_other_bytes_and_author`). The
projection also never writes, so nothing here can repair the host's log even in
principle.

This finding is conditional on the shape, and it is worth restating when the
shape changes. If `SessionMessage` ever grows a field that names another
message — a tool-call id, a structured citation, an edit or redaction pointer —
then the pairing OpenBot lost becomes representable here, and a read-side
repair fold belongs in this module, written to OpenBot's rules: an ordered rule
list, ids never rewritten, unchanged messages returned as they were, and the
host's log never touched.

## Invariants and constraints

- The host owns every durable row and cursor.
- The runtime owns no database, file, socket, transport, or model client.
- The core crate remains synchronous and runtime-free.
- One source row produces at most one attributed session message.
- No projected message names another message, so a projection cannot leave a
  dangling reference and needs no repair pass. A reply whose root fell outside
  the scan is dropped rather than flattened.
- Page validation prevents non-advancing walks and duplicate output.
- Briefing order is deterministic and follows effective desk or roster order.
- The briefing never offers a capability the run cannot use. Mention dispatch
  is stated only under a supplied context that reports `may_dispatch`.

## Acceptance criteria

- Tests cover zero-window reads, exclusive bounds, multi-page ordering,
  malformed pages, duplicate rows, non-advancing and empty-page cursors, the
  scan cap, desk filtering, channel and thread projection, root termination,
  blank-content skipping, current-message exclusion, and attribution.
- Tests cover briefing filtering, order, deterministic text, General behavior,
  initialization separation, and projection error propagation.
- Tests cover the withheld and offered dispatch renderings: no context, a
  disabled policy, a zero budget, a hop at and past `max_hops`, and a run
  inside its budget, whose text is pinned in full.
- Public payload serde forms, rustdoc examples, workspace contracts, purity,
  rustdoc, and doctests pass.

## Open questions

None for P4. Watermark-based continuous sharing begins in P5.
