# Trace

The stigmergic grammar: what a message deposits into the shared transcript,
and how it is read back. Coordination here is stigmergic in Grassé's sense —
work leaves a trace in a shared medium, and the trace is the stimulus for the
next unit of work. The transcript is the medium; no agent addresses another,
and nothing in this module dispatches anything.

The grammar is specified in [`docs/specs/grammar-traces.md`][spec]; this
module is its implementation.

## Design

One line, recognised only at the start of a line (leading whitespace
ignored) and only outside code:

```text
!<kind> [#topic] [>target] [^cite ...] [free text]
```

`resolve` is the entry point:

- `None` for `supplied` **extracts** every marker line from `body` in reading
  order, up to [`TRACE_CAP`].
- `Some(supplied)` **revalidates** — it selects among what extraction finds by
  `(offset, kind)` only. Anything else a supplied trace claims (a different
  topic, target, citation or text) is discarded in favor of what the body
  actually says, and a repeated offset is rejected rather than selected
  twice. There is no host-side resolution step here the way there is for
  `mention::resolve`, so a supplied entry can only *pick*, never invent.

[`read`] folds a whole projected transcript into traces, in sequence order,
for a host or for the episode itself; [`read_borrowed`] is the same fold over
borrowed messages, for the episode's own hot path, which filters the
transcript on every step and cannot afford to clone it each time.

### Code and asides are both masked

Extraction shares [`masking::code_ranges`][code_ranges] with the mention
grammar, so a span this crate reads as code is the same span the mention
grammar reads as code — one scanner, one answer, for both.

A message addressed to an aside contributes nothing to `read`, for a
different reason: **an aside carries information, never support.** A marker
written where the room cannot read it adds no supporter, silences no
advocate, and earns no directory credit, identically for every reader. See
[ADR 0010][adr-0010]. The filter lives here, in the one place a transcript
becomes a medium, rather than only inside the episode — a host folding its
own standings from the same transcript must get the same answer `step` does,
which is what keeps quorum single-valued.

### Two markers fail closed

`!refute` requires both a `#topic` and at least one `^cite`; `!defer`
requires a `#topic`. Missing what each needs, the line yields **no trace at
all** — never a trace that could cap a topic on nothing, or abstain from
nothing in particular. Every other marker degrades gracefully (an unnamed
topic, target or citation is simply absent); these two do not, because their
whole effect depends on naming something real.

## Public surface

| Item | Purpose |
| --- | --- |
| `resolve` | Read traces from a body, or revalidate a supplied list against it. |
| `read` | Fold a projected transcript (`&[SessionMessage]`) into traces, in sequence order. |
| `TRACE_CAP` | Maximum traces read from one message body (16); extras beyond this in reading order are dropped. |
| `Trace` | One typed deposit: `sequence`, `author`, `kind`, `topic`, `target`, `cites`, `text`, `offset`. |
| `Trace::grounded` | Whether the trace carries at least one citation. |
| `Trace::agent_id` | The author's canonical id, when an agent (not a person, operator, or system) authored it. |
| `TraceKind` | `Propose`, `Support`, `Object`, `Refute`, `Evidence`, `Question`, `Commit`, `Defer` — see each variant's rustdoc for what it does to the deliberation. |
| `TopicId` | A proposal identity that support and objection attach to. |

`read_borrowed` is `pub(crate)`: it is the same fold as `read` over `&[&SessionMessage]`,
kept out of the public surface because it exists only to save an allocation on
the episode's own hot path.

## Operational constraints

- **Order-independent within a message, order-preserving across one.**
  `read` sorts by `(sequence, offset)`, so a redelivered or reordered message
  cannot double a trace or move it out of authored order.
- **Fails closed, never open.** An unrecognised marker word, a malformed
  `#topic`/`>target`/`^cite`, or a missing requirement on `!refute`/`!defer`
  all yield no trace for that line — never a trace with a guessed field.
- **A body with no `!` short-circuits.** Extraction does not scan or allocate
  for the common case of ordinary conversation.

[spec]: https://github.com/tinyhumansai/tinyhivemind/blob/main/docs/specs/grammar-traces.md
[adr-0010]: https://github.com/tinyhumansai/tinyhivemind/blob/main/docs/adr/0010-an-aside-carries-information-never-support.md
[code_ranges]: tinyhivemind_core::masking::code_ranges
