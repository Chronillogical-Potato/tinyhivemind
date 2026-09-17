# `responder`

The runtime boundary for one optional, tool-less responder-selection call.

## Why it exists

Choosing who answers a message is mostly pure — the responder ladder in
`tinyhivemind-core` already resolves an explicit mention, a single-member desk, or
a disabled selection policy without waiting on anything. The one rung that
cannot be pure is auto-selection: it asks an evaluator for a typed candidate
distribution. This
module is the whole waiting part of that ladder, held to one call, at most
once, with no transcript, no tools, and no host handles reachable from inside
it.

## Public surface

| Item | What it is |
| --- | --- |
| `Selector` | trait a host implements to return a typed distribution over one `SelectionRequest` |
| `SelectorFuture<'a>` | the boxed, executor-neutral future `Selector::select` returns |
| `BoxError` | a boxed failure returned by a host selector implementation |
| `choose_responder(selector, request, roster, desks, candidate_details)` | run the ladder, calling `selector` at most once |
| re-exported from `tinyhivemind_core::responder` | typed candidates, fixed-point probabilities, `SelectionEvaluation`, `accept_evaluation`, and the responder ladder payloads |

`responder_plan` (pure, in `tinyhivemind-core`) does the actual ladder walk and
returns either an already-`Decided` decision or a `Select` request describing
exactly what to ask a model. `choose_responder` is the thin async shell around
it: call the selector if one exists and the plan asks for it, validate the
distribution, and fall back on absence, failure, low confidence, or an invalid answer.

## Constraints worth knowing

- **At most one call, ever.** `choose_responder` invokes `selector.select`
  exactly once when the pure plan requires selection, and not at all otherwise
  — an already-decided rung, a `None` selector, or an empty candidate list all
  short-circuit before any call is made. This mirrors the charter's one
  message, one turn rule at the responder layer: auto-selection asks a model
  once and accepts what comes back or falls through.
- **Selector failure is not a runtime error.** A future that resolves to
  `Err` is treated exactly like invalid output: it moves to the desk-default
  fallback with an appropriate `SelectionDisposition`, rather than propagating
  through `Result`. Only a reached fallback with no active responder, or a
  typed pure-algebra failure from snapshot validation, is a real `Error`.
- **The trait is object-safe and borrow-friendly.** `Selector::select` takes
  `&'a self` and `&'a SelectionRequest`, and its returned future shares that
  lifetime, so an implementation may hold either borrow across an `.await`
  without forcing an owned clone. `BorrowingSelector` in the tests exercises
  this directly.
- **The selector sees no transcript and no tools.** `SelectionRequest` carries
  only the raw message, the canonical desk id, and the bounded effective
  candidates the pure core already assembled — nothing here hands a selector a
  host handle, a log reader, or a way to act.

## Where the result goes

`ResponderDecision` is the runtime's answer to "who replies to this message";
a host reads its `responder_id` and `rung` to route the next turn, and its
`disposition` to decide whether to log or surface a fallback.
