# Integration tests

The regression suite for this crate's public contract: every file here
imports through the published surface only, exactly as a host would, and
`support/` supplies the one thing a real host and a test have in common — a
journal and a set of participants — without pulling in a network or a model.

## Files

| File | Covers |
| --- | --- |
| `public_api.rs` | Every item re-exported from the crate root and its submodules is reachable and behaves as documented from outside the crate. A change that moves an item out of the re-export surface fails here rather than in a host. |
| `hive_episode.rs` | End-to-end deliberation through `HiveHarness`: convergence, one-turn-per-step, budget exhaustion, the blind opening round, deadlock reporting, cross-inhibition breaking a tie, refutation killing a hypothesis, expert delegation (`Knows`) surfacing an uncited fact, and that an episode does not inherit votes from before it opened. |
| `fuzz_invariants.rs` | Deterministic fuzz-style property tests (a fixed xorshift generator, not a proptest dependency — this crate adds none) over arbitrary transcripts: `standings`, `directory` and `bids` are stable, well-formed and idempotent under reordering and redelivery; private asides interleaved anywhere never move the episode; exchange rounds terminate inside their budget without moving it either. |
| `openrouter_hive_live.rs` | Opt-in coverage driven by real models over OpenRouter. Asserts **structure, not quality**: parseable traces, termination inside budget by one of the four terminal steps, one agent per turn, attribution preserved. Nothing here claims the room reached a good answer. Gated behind `live_tests_enabled` — see below. |
| `support/hive_harness.rs` | `HiveHarness`: a small in-memory host owning its own append-only journal, desk and roster, driving `step` to termination and appending each authorized turn. This is the minimal host contract — fold, run the one authorized turn, append it, commit the returned state, repeat — written once so every other test file in this directory can reuse it instead of restating it. |
| `support/scripted_agent.rs` | `ScriptedAgent`: a deterministic `HiveAgent` that replays a fixed script of lines and falls back to a filler question, or to a self-computed `!commit` once the room can see quorum, when its script runs out. Records exactly which sequences each of its turns was shown, for assertions about what a turn could and could not see. |

## Running the live suite

`openrouter_hive_live.rs` is excluded from an ordinary `cargo test` run unless
its gate is satisfied — the file's own `live_tests_enabled` checks for the
environment the harness needs (an API key and base URL) and the test
early-returns rather than failing when it is absent, so CI and a
network-less local run both pass without special flags. Point it at a real
endpoint to exercise it; see the file's module doc for exactly what it does
and does not assert.

## Operational constraints

- **Public-API-only.** No file in this directory reaches into a private
  module of `tinyhivemind_core::hive`; `support/` is the one place that imports
  crate-internal test-only conveniences (`quorum`, `trace::read`, and the
  library's own `HiveAgent`-shaped participant loop), and it does so through
  the same published paths a host would use.
- **No host types leak in.** `HiveHarness` is itself a minimal host — it owns
  the journal, the roster and the desk set — never a stand-in for a
  consuming application's types, matching the charter's "no host types,
  ever."
- **Deterministic by default.** Every test except the live one drives fixed,
  scripted or seeded-xorshift participants, so a failure here reproduces
  exactly and is never a network flake.
