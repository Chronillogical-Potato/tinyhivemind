# Implement typed System One decisions

Linked specification: [`../specs/jev-integration.md`](../specs/jev-integration.md).

1. Implement the existing P16 approval specification and its one runtime port.
2. Replace selector text with checked fixed-point distributions and confidence.
3. Add source-bound Choice, Score, and Noul decision snapshots to quorum and a
   `step_with_evaluations` transition path.
4. Add the native Jev adapter as an example-only dependency; keep transports
   out of library dependency graphs.
5. Add the paired strict-JSON baseline, metrics table, and diagnostic output to
   the existing benchmark.
6. Pin serde forms, cover failure paths, run the four workspace contract
   commands, purity check, rustdoc, doctests, benchmark self-check, and gated
   live evaluation.
