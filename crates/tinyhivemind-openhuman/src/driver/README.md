# Completion driver

`mod.rs` returns bounded pending OpenHuman agents and folds only host-committed
utterances. Serializable `DriverState` retains exact replay identity, a
required monotonic revision, accepted scheduling order, and global freshness history across reconstructed
drivers. The persisted freshness floor is validated against the episode
watermark, every participant assignment and completion sequence, and every
receipt. A `PendingRound` is an opaque proposal bound to the exact state
snapshot that created it, so it cannot be forged or reused after that state
advances. Batch application validates a whole committed round before invoking
any semantic router. A batch containing only distinct exact receipt matches is
recognized without consulting the pending round, which makes replay safe after
a restart even when the recomputed round is empty or different. It emits no new
host action. Committed rounds are folded in
host-sequence order. Each broadcast fallback is the next distinct episode
participant in current scheduling order, wrapping at the end; the whole batch
is rejected before routing if any author has no valid fallback. Broadcast
candidates likewise exclude the author, and policy is clamped to the driver's
round width. A broadcast run action retains the exact accepted
`RoutingPlan`, so the host can persist its existing routing audit without a
second model call. `test.rs` and `../../tests/review_regressions.rs` pin wire
forms, stale and mismatched rounds, duplicate sequences, privacy, ordering,
bounds, participant routing, and replay behavior.

`order.rs` keeps accepted recipients ahead of remaining participants while
deduplicating and pruning completed work. `test/broadcast_fallback.rs` covers
per-author fallback behavior separately from the general tests in `test.rs`.
