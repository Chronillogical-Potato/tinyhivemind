# Integration tests

Everything here links against `tinyhivemind-core` as a downstream consumer
would — only what `src/lib.rs` re-exports is reachable. `crates/<name>/src/`
holds the module-local unit tests that may touch private items; this
directory is the regression suite for the crate's *public contract*. If a
change here breaks, it is a breaking change for users.

## Files

- `public_api.rs` — exercises the full public surface (chat identity, desk
  overlays, roster, mention resolution, dispatch) through the same imports a
  consumer would write. This is the file to extend whenever a new item is
  exported from `src/lib.rs`.
- `fuzz_invariants.rs` — deterministic, seeded fuzz-style regression cases for
  mention resolution over arbitrary Unicode and punctuation-heavy bodies. It
  runs under the normal test suite (`cargo test`), so parser invariants are
  exercised in CI without a separate fuzzer toolchain.
- `coordination_harness.rs` — end-to-end and edge-case tests for a host
  coordinating multiple agents over one shared transcript, built on
  `support::harness` and `support::scripted_agent`.
- `openrouter_live.rs` — opt-in live coverage against a real `OpenRouter`
  model. Gated behind the `e2e` feature and further gated at runtime behind
  `TINYHIVEMIND_LIVE_OPENROUTER`; skipped by default and by CI.
- `support/harness.rs` — `ChatHarness`, a small in-memory host that owns an
  append-only journal and dispatches one agent turn per call, plus the
  `AgentEngine` trait and `Message` record the harness drives. Deliberately
  mirrors the same host/library boundary a production consumer has: the
  library under test contributes only conversation identity
  (`same_conversation`), and the harness owns everything else.
- `support/scripted_agent.rs` — `ScriptedAgent`, a deterministic `AgentEngine`
  that plays back a fixed queue of responses and records every transcript it
  was handed, for asserting exactly what context each turn saw.

## Conventions

- A file that needs the shared harness declares it with `#[path = "..."]`
  rather than a shared `mod.rs`, since each integration test file compiles as
  its own crate root; see `coordination_harness.rs` and `openrouter_live.rs`.
- Live/network tests are named `live_*` or gated behind a feature plus an
  env-var switch (see `openrouter_live.rs`) so they can be excluded without
  touching the default suite.
- `#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]` is
  applied per-file where tests need it; it is a test-only allowance and does
  not relax anything in library code.

The `migrated_runtime/`, `migrated_hive/`, `migrated_embed/`, and
`migrated_driver/` directories preserve the former crates' public contract
suites and fixtures. The corresponding prefixed top-level `.rs` files compile
each suite as an integration-test target of core.
