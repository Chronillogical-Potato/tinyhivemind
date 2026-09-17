# Implement typed System One decisions

Linked specification: [`../specs/jev-integration.md`](../specs/jev-integration.md).

## Goal and constraints

Replace prompt-and-parse control decisions with typed fixed-point snapshots,
while keeping transports and model calls outside the pure library crates. Jev
selects, scores, and verifies; application code owns thresholds, policy, state
transitions, and every free-form worker response. The transcript-only fold
remains the deterministic compatibility path.

## Implementation sequence

1. **Approval algebra and runtime gate — complete.**
   - Add failure and wire tests in
     `crates/tinyhivemind-core/src/approval/test.rs` and
     `crates/tinyhivemind/src/approval/test.rs`.
   - Implement the total fold under `tinyhivemind-core/src/approval/` and the
     async host port under `tinyhivemind/src/approval/`; export them from each
     crate root and align their module READMEs.

2. **Typed responder selection — complete.**
   - First add invalid-distribution, confidence, fallback, and serde tests in
     both crates' responder test modules.
   - Implement bounded `Probability`, complete candidate distributions, and
     `SelectionEvaluation` in the core and runtime responder modules.

3. **Probability-weighted quorum — complete.**
   - Add source-binding, admission, freshness, duplicate, composition, public
     API, and episode-wire tests under `tinyhivemind-hive/src/quorum/test/` and
     the hive integration suites.
   - Implement `DecisionEvaluation`, `standings_with_evaluations`, and
     `step_with_evaluations` in the hive quorum and episode modules without
     changing the original `standings` and `step` behavior.

4. **Native Jev adapter — complete.**
   - Pin `tinyjevclient` as an example-only dev dependency in `Cargo.toml` and
     `crates/tinyhivemind-hive/Cargo.toml`; keep normal/build trees pure.
   - Add routing Choice, stance Choice, evidence Score, violation Noul, label
     validation, and approval narrowing in `examples/bench/jev.rs`, with tests.

5. **Paired evaluation harness — complete.**
   - Add CLI, schema, corpus, metric, and escaping tests under
     `examples/bench/decision_eval/` before wiring live calls.
   - Run identical cases through OpenRouter strict JSON and Jev, alternating
     request order and independently bounding provider concurrency. Report
     latency, throughput, tokens, cost, failures, accuracy, and calibration.
   - Record the campaign in
     `docs/experiments/2026-09-17-jev-decision-evaluation.md`.

6. **Documentation and verification — complete.**
   - Align `README.md`, `ROADMAP.md`, module READMEs, specs, rustdoc, and wire
     forms, then run focused tests and every full command below.

## Verification

```sh
cargo fmt --all -- --check
cargo clippy --all-targets --all-features -- -D warnings
cargo build --all-targets --all-features
cargo test --all-features
.github/scripts/assert-pure.sh
RUSTDOCFLAGS="-D warnings" cargo doc --no-deps --all-features
cargo test --doc
cargo run -p tinyhivemind-hive --example bench -- --stats-check
```

## Completion checklist

- [x] Approval decisions and the host gate cover every failure path.
- [x] Selector output is typed, bounded, and schema-pinned.
- [x] Weighted quorum and evaluated transitions are deterministic.
- [x] Jev remains example-only and pure dependency graphs remain clean.
- [x] The paired 1,000-case campaign and diagnostics are recorded.
- [x] Workspace, docs, purity, and benchmark self-checks pass.
