# Seat continuity: the fold a seat carries, and the feedthrough it emits

**Status:** Draft — first slice implemented in the `desk` example, nothing in
the library yet
**Owner:** tinyhivemind maintainers
**Reading:** [`../research/multi-context.md`](../research/multi-context.md)
**Evidence:** [`../experiments/2026-09-09-desk-lessons.md`](../experiments/2026-09-09-desk-lessons.md)

## Problem

A seat on a desk is a model with one bounded window. Between turns, the desk
example runs it as a fresh process, so everything the seat reasoned during a
turn is gone when the turn ends. What survives is the ten-row window, the
pinned board, and the shared workspace — and the workspace is where the seats
have been storing the continuity they lack, as a 37k-character `NOTES.md`
that is every seat's and therefore nobody's.

The measured costs, from runs 21–27:

- one message every 19.4 minutes against the Grok desk's 2.1, with the
  one-process-per-turn boundary the known cause: turn 1 of `@solver` spent 15
  of 19 tool calls re-reading 12 files before doing anything;
- a verified result that reached the room as three characters and was
  recovered only because the seat had also written a file, and the chair's
  nudge happens to name that file;
- five copies of the task brief in a ten-row window after five restarts.

The fix that was tried — resuming the CLI session — made the request payload
grow without bound until every call stalled. It is off, and the comment in
`run.rs` says why.

The desk does not need an orchestrator that holds everything. The reading in
[`multi-context.md`](../research/multi-context.md) is that no shipping design
has one; they have a medium that outlives any window, a thin controller, and
seats that carry a **fold** of their own work between turns. This workspace
has the first two. This specification adds the third, and two small
awareness mechanisms the same evidence asks for.

## Goals

1. A seat carries its own working state across turns without its process or
   its CLI session surviving, and without the request growing turn over turn.
2. The room learns what a seat wrote to the workspace without the seat having
   to say so.
3. A restart does not spend window rows on a brief every seat already holds.

## Non-goals

- Adding messages to one turn. A seat still posts exactly one message per
  turn, and only its first direct mention dispatches.
- A model in the chair. The controller stays deterministic.
- A second journal. The fold is superseding per-seat state the host owns, like
  `SharingState`; it is not a log and it is not in the log.
- Sharing the fold. It is the seat's own; peers learn decisions from posts.
- A larger window.

## Proposed behavior

### The notebook

Each seat owns one notebook: a text the host persists per `(desk, seat)`, that
the seat **rewrites** before it posts and the host **reads back verbatim** at
the top of the seat's next turn.

- **Location.** Host-owned. The desk example uses
  `<workspace>/notebooks/<seat-id>.md`, written with the seat's own file tool
  so that no new model-authored delimiter is introduced — every delimiter is a
  fallible interface, and the desk already paid for one.
- **Semantics.** Superseding, not appending. The seat is told to write the
  notebook as the message it would want to receive from itself next turn:
  what it established, what it is mid-way through, what it would do next, and
  which files hold what. A notebook that only grows is the resumed-session
  failure in a file.
- **Budget.** `NOTEBOOK_CHARS`, 6000 in the example. Stated to the seat in the
  prompt. When a notebook overruns, the host keeps the **tail** — the newest
  writing is at the bottom of a rewritten file, and a seat that appended
  instead of rewriting has its oldest material fall off — and prefixes one
  line saying how much was dropped, so the seat can see its own overrun.
- **Placement.** Under a heading the seat recognises as its own, after the
  briefing and standing brief and before the room's transcript. It is the
  seat's prior context, so it is read where prior context would have been.
- **Privacy.** Only the owning seat's prompt carries its notebook. It is on
  disk, so a person can read every notebook; it is a deliberation device and
  not a security boundary, the same stance `Audience::admits` takes.
- **Absence.** A seat with no notebook yet is told so, in one line, and told
  to start one this turn.

### Feedthrough rows

When a turn's event stream shows the seat wrote or edited files, the host
appends **one** row after the seat's post:

```text
author:   SessionAuthor::System { kind: "workspace", label: "workspace" }
audience: Audience::Desk
content:  @solver wrote psi_sublinear.py, NOTES.md
```

- File names are base names, deduplicated, in first-write order.
- The seat's own notebook is excluded: it is private, and announcing it would
  invite peers to read it.
- No row is written when nothing was written. The row is a fact about the
  artifact, never a summary of the post.
- One row per turn, so the cost to every peer's window is bounded by the turn
  count, and it is deliberately terse: it is a pointer, not content.

### The brief stays pinned

The chair's opening message is appended **only when the transcript is empty**.
On a resumed transcript the opening is already the first row, and the standing
brief in every seat's prompt carries the task regardless of what has scrolled.
The first turn of a resumed run is still triggered by the brief; it is simply
not appended again.

## Invariants and constraints

- The feedthrough row is authored by the system, carries no mention, and can
  dispatch nothing.
- The transcript remains append-only and never condensed. Nothing here edits a
  row or removes one.
- The library holds no notebook. It is host state. Should the fold move into
  the library, it goes in as a `BriefingNote` producer with a stated budget —
  the same place `SessionContext.notes` already reserves for "board state,
  open work, anything a host wants a turn to know and this crate cannot
  compute" — and never into the log.
- A notebook is never quoted to a peer, never searched by `search_messages`,
  and never pinned: it is not in the log, so none of those can reach it.
- The host never rewrites a notebook. Truncation on read is a projection; the
  file is left as the seat wrote it.

## Acceptance criteria

Observable on a run of the `desk` example against the same task and desk file
as runs 26–27:

1. The second turn of any seat contains a `## Your notebook` section whose text
   is byte-identical to the tail of `notebooks/<seat>.md` as it stood after
   that seat's previous turn.
2. Per-turn `read` tool calls on workspace files fall from the run-27 figure
   of 15 in the solver's first turn; the number is printed per turn.
3. Every turn that wrote a file is followed by exactly one `system/workspace`
   row naming those files, and a turn that wrote none is followed by none.
4. Restarting a run on an existing transcript adds zero rows before the first
   agent turn.
5. `cargo fmt --all -- --check`, `cargo clippy --all-targets --all-features
   -- -D warnings`, `cargo build --all-targets --all-features` and
   `cargo test --all-features` pass.

Criterion 2 is the one this is for, and it is a measurement rather than a
threshold: the number is recorded and compared, not asserted.

## What this does not yet do

- **Deltas.** A seat that carries a notebook could be handed only what is new
  since its last turn — `prepare_delta` exists for exactly this — instead of
  the full window. Not done here, because the measured cost is tool-loop
  re-reading, not prompt size, and a change to what a seat *sees* of the room
  should be measured on its own.
- **A library type.** Deliberately. The algebra already holds every shape this
  needs; the defects were in the host, and the fix is measured in the host
  before anything is promoted.
- **Cadence.** The 19.4 minutes is expected to fall with the re-reading, but
  the turn safeties that failed in run 27 are unexplained and unfixed, and
  the lessons file is explicit that the turn loop is instrumented before it is
  tuned. That comes first.

## Open questions

- **Feedthrough rows spend the window.** In the offline smoke run every turn
  wrote a file, so every agent row was followed by a workspace row and a
  ten-row window held five agent messages. Against real seats most turns
  write, so this is the expected shape. The options are to count the window
  in agent rows rather than rows, to widen the window by the feedthrough
  count, or to accept that a pointer to the artifact is worth the row it
  costs. Measure before choosing; the smoke run cannot say which.

- Should the notebook budget count against `BrevityPolicy`, or is it its own
  budget? It is not in the window, so today it is separate.
- Is the tail the right thing to keep on overrun? For a seat that rewrites,
  the file has no age gradient and the choice is arbitrary; for one that
  appends, the tail is newest. The alternative is to refuse to inject an
  overrun notebook at all and tell the seat, which is stricter and may be
  right.
- Should a feedthrough row name files *read* as well as written? Reads are the
  cost being measured; publishing them would let a peer see redundancy, at
  the price of a longer row.
