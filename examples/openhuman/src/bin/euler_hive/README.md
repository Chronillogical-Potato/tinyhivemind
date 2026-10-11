# euler_hive

A live hive of OpenHuman agents defined entirely by the
[`euler-hive`](../../../../../crates/tinyhivemind-lang/examples/hives/euler-hive/README.md)
hive-language package. The host loads the package from disk, replaces its
placeholder `context/problem.md` with the statement it is handed, parses and
lowers it, and turns every lowered seat into an agent through the language
adapter. Nothing about the hive is written in this binary: desks, seats,
personas, tool access, topology, referral policy and memory bindings all come
from the package.

```sh
set -a; . /path/to/.env; set +a        # OPENROUTER_API_KEY, TINYHUMANS_API_KEY
cargo run --manifest-path examples/openhuman/Cargo.toml --bin euler_hive -- \
  --problem hive-runs/pe1014/problem.html --problem-id pe1014 --runs hive-runs \
  [--smoke] [--model <openrouter id>] [--max-turns N] [--max-spend-usd X] ...
cargo run --manifest-path examples/openhuman/Cargo.toml --bin euler_hive -- \
  --probe --problem <statement.html> --runs hive-runs   # dependencies only, no model call
cargo test --manifest-path examples/openhuman/Cargo.toml --bin euler_hive
```

`--runs` must be outside the repository (`hive-runs/` at the main checkout's
root is git-ignored): a run directory holds the problem, every transcript and
program, and the hive's answer, none of which may be committed.

## What runs where

| Concern | How |
| --- | --- |
| Inference | OpenRouter, every seat (`OPENROUTER_API_KEY`). Default `google/gemini-3.8-flash`: a reasoning model with native tool calls at about 100 tokens a second; `OPENROUTER_MODEL` or `--model` overrides. |
| Seats | `language::management_request(seat)` into `SeatFactory::create_with_memory`; each agent is `ToolScopeSpec::HostOnly` with the seat's prompt, role charter and the hive protocol as its whole (bare) system prompt. |
| Tool gate | The seat's `tools.allow` is its whole belt (`package.rs`); an unknown or denied name refuses the seat. Host-only means no harness tool (shell, files, fetch, memory) exists for the model. |
| Web search | Research seats only: the TinyHumans backend's agent integrations through the TinyHumans SDK (`TINYHUMANS_API_KEY`): Parallel search and Gemini with Google Search grounding. Exa and TinyFish are wired but the backend refuses API-key callers there (`--probe` shows it). A query that quotes the statement's own data (numbers of five or more digits, upper-case words of five or more letters) or looks the problem's source up is refused before it is sent: seats search for the mathematics, not the problem. |
| Python | Compute and verify seats only: one fresh `python:3.12-slim` container per program, `--network none`, read-only root, 64 MB tmpfs `/tmp`, 1 GB, 2 CPUs, 128 pids, all capabilities dropped, unprivileged user, 120 s, program on stdin, **nothing mounted**. |
| Memory | CortexDB behind the TinyHumans backend (`CortexEngine::tinyhumans`), installed as the runtime's memory engine. `language::memory_binding` binds each agent, so OpenHuman logs its turns under `<root>/agent:<seat>`; `SeatMemory` honours the rest of the binding (moments, reads, budget, kinds, read-only, the verified pool) as core `Recall`/`Remember`. `lifecycle: run` is a `ws:<run>` scope under the package root. |
| Desks | One floor per desk under core's `CompletionDriver`; broadcasts routed by core with `MentionRouter`; desks reach each other only through core's `referral` fold along declared links (`floors.rs`, `referrals.rs`). Each piece of work on a desk is a *chain* with its own id: a question it asks owes the chain one answer, which reopens the chain on the asking seat when it runs, and a chain reports only when nothing it asked is still owed, so overlapping questions on one desk cannot lose each other's answers. |
| Budgets | Seat turns, model calls, wall clock, sandbox runs, searches and dollars, each checked before the spend it guards. Spend is the larger of OpenHuman's per-turn cost and OpenRouter's own account delta for the key. |
| Secrets | Keys come from the environment only and reach no argv, prompt, artifact or log. OpenHuman's secret store is in memory: an embed host installs no OS keyring provider, so a run never asks the keychain and nothing it stores outlives the process. |

## Artifacts

`<runs>/<problem-id>/<timestamp>/`: `frames.ndjson` (the `hive_view` wire
contract, flushed per frame, so the lab viewer can follow a live run; adds
`turns[].tool_calls`, `turns[].memory`, `turns[].usage`, `start.limits`),
`structure.json` (what `hive_view <package> --json` prints), `events.ndjson`,
`turns/` (each turn's prompt, reply and action), `sandbox/` (each program and
its output), `search/` (each query and result), `package.json`,
`TRANSCRIPT.md` (every desk's journal) and `report.json` (ending, answer,
spend, referrals, and per-seat memory counts read back from CortexDB).

## Files

| File | Purpose |
| --- | --- |
| `package.rs` | Read the package directory, inject the problem, parse and lower; each seat's belt |
| `agents.rs` | The seat factory behind `management_request` |
| `turn.rs` | One seat turn: recall moment, prompt, belt, one hive action, host compaction |
| `tools.rs` | The native tools: `web_search`, `run_python`, the three speaking tools, `hive_remember`, `hive_recall` |
| `floors.rs` | One `CompletionDriver` floor per desk |
| `referrals.rs` | Asks, reports and returns through core's `referral` fold |
| `router.rs` | `MentionRouter`: a broadcast reaches the colleague it names |
| `memory.rs` | `SeatMemory`: the binding contract over CortexDB, run scoping, store counts |
| `sandbox.rs` | The Docker `run_python` sandbox |
| `search.rs` | The TinyHumans SDK search client |
| `budget.rs` | Limits, spend meters, OpenRouter account usage |
| `frames.rs` | The viewer's frame types |
| `artifacts.rs` | The run directory |
| `report.rs` | `structure.json`, `--probe`, closing frames, `report.json` |
| `test/` | Offline tests, one module per behaviour: package wiring, sandbox and search, memory, routing and prompts, federations, artifacts |
| `request_test.rs` | Real seats on a real runtime against a loopback model: each request carries exactly the seat's allowed tools |
