# Completion driver

`mod.rs` returns bounded pending OpenHuman agents and folds only host-committed
utterances. Serializable `DriverState` retains exact replay identity, a required
monotonic revision, accepted scheduling order, global freshness history, and --
beside the episode fold -- a `Ledger` of what the fold cannot record: handoffs
queued for a busy recipient, broadcasts charged to each assignment, and
questions whose answers are still owed. A `Seen` record holds what the host has
reported about delivery and turns; it moves the wake predicate, and a pending
round stays valid across it.

The discipline the ledger enforces, in order:

- **Complete, then drain, then assign at the completion's sequence.** A
  completion records the work the seat did; the queued handoff it frees is
  assigned at that same row, which exists and is above the record just closed.
  Assigning at the broadcast's origin would be satisfied by the seat's previous
  work; assigning at a future row would sit above everything the watermark can
  reach.
- **A broadcast always resolves to an owner.** Each routed recipient is
  assigned now or queued; a full queue skips that recipient, and if nobody
  could take the work it stays with its author, who is owed another turn.
- **An open question holds completion.** A seat that asked may not complete
  until every seat it asked has committed a row after the ask; a row from the
  asked seat is the answer. A settled seat that owes an answer is woken for it.
- **A budget is admitted before the model is called**, so a refusal costs
  nothing. It resets by comparison: a new assignment reads as unspent.
- **The episode is over when it is quiescent** -- complete, nothing queued,
  nothing awaited -- not when its status first reads complete.

A `PendingRound` is an opaque proposal bound to the committed state that
created it, so it cannot be forged or reused after that state advances. Batch
application validates a whole committed round before invoking any semantic
router; a batch containing only distinct exact receipt matches is recognized
without consulting the pending round. Committed rounds are folded in
host-sequence order. Each broadcast fallback is the next distinct episode
participant in current scheduling order, wrapping at the end. Broadcast
candidates exclude the author, and policy is clamped to the driver's round
width. A broadcast run action retains the exact accepted `RoutingPlan`.

| file | holds |
| --- | --- |
| `mod.rs` | `DriverState`, `CompletionDriver`, `HostAction`, start/resume validation, the per-event fold, completion |
| `ledger.rs` | `Ledger`, `Seen`, `Handoff`, `AssignmentSpend`; queue, budget and open-ask bookkeeping |
| `brief.rs` | `EpisodeBrief`, `Channel`, `ConversationView`, `standing_contract`: what the episode tells a seat before a turn, for a host to prepend its own context to |
| `broadcast.rs` | routing one broadcast and placing it: assign, queue, or return to author |
| `round.rs` | folding a whole pending round: replay recognition, validation, preflight |
| `order.rs` | accepted scheduling order, the wake predicate, and the stalled set |
| `test.rs`, `test/` | wire forms, stale and mismatched rounds, duplicate sequences, privacy, ordering, bounds, routing, replay, and the ledger's ordering rules |

`../../tests/review_regressions.rs` pins the wire form and the resume checks;
`../../tests/fuzz_invariants.rs` asserts the properties that must hold under
arbitrary event orderings.
