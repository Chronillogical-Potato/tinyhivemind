# harbor

A Harbor (Terminal-Bench) custom agent that runs the SWE hive or the
single-agent baseline. The model is called by the compiled `swe_hive` binary on
the host; the task sandbox has no internet. Commands come back over a JSON-lines
pipe and run with the task `environment.exec`.

| File | Purpose |
| --- | --- |
| `hive_agent.py` | `HiveAgent` (`BaseAgent`), kwargs `mode=hive|single` and more |
| `selftest.py` | runs the agent against a local fake environment and the mock model |

## Run

```sh
cd examples/lab && cargo build --release --bin swe_hive
export OPENROUTER_API_KEY=...        # host only; never passed on the command line
PYTHONPATH=$PWD/harbor harbor run \
  -d terminal-bench-sample@2.0 -l 1 \
  --agent hive_agent:HiveAgent \
  -m openai/gpt-oss-120b:nitro \
  --ak mode=hive            # or mode=single
```

This Harbor has no `--agent-import-path` flag: `--agent module:Class` is the
import path, and the module must be on `PYTHONPATH`. Other kwargs, all
optional: `--ak bin_path=...` (else `$SWE_HIVE_BIN`, then `target/release`, then
`target/debug`), `api_base`, `max_turns` (60), `round_width` (2), `token_cap`,
`steps_per_turn` (12), `cmd_timeout` (180), and the context policy below.

## Context policy (fair baseline)

The single seat keeps one conversation, so on long tasks (8h agent timeout,
~131k-token model) it would overflow for reasons unrelated to the hive idea.
The policy is explicit and measured:

| Kwarg (`--ak`) | Binary flag | Default | Meaning |
| --- | --- | --- | --- |
| `single_context` | `--single-context` | `mask` | `none`: grow unbounded; an overflow error ends the run as `aborted: "context_overflow"`. `mask`: once the last prompt exceeds the budget, replace the body of old tool results with `[output elided: N bytes, cmd=...]`, keeping the newest `context_keep`; assistant messages and commands stay; zero extra tokens. `summarize`: one extra metered call condenses the oldest half into a note. |
| `context_budget` | `--context-budget` | 60000 | prompt tokens above which the policy acts |
| `context_keep` | `--context-keep` | 8 | tool results `mask` leaves whole |

Hive seats run under a hard `mask` at the same budget (their briefing is already
bounded, so it normally never fires). `result.json` carries `context_policy`,
`context_budget`, `context_events` and `max_prompt_tokens` (the largest prompt of
any model call) for both arms; the trace has a `mark` labelled `context` each time
masking or summarizing fires and `context_overflow` on an overflow abort.

Compare arms on the same tasks by running once with `mode=hive` and once with
`mode=single`, each with its own `-o` jobs directory.

## Outputs

Per trial, in the agent log dir: `trace.jsonl` (open it in `../viewer`),
`result.json`, `instruction.txt` and `swe_hive.stderr`. The `AgentContext`
carries `n_input_tokens` and `n_output_tokens` from the same meter, with the
full summary in `metadata`.

## Verify without spending money

```sh
PY=$(dirname $(dirname $(readlink -f ~/.local/bin/harbor)))/bin/python
$PY examples/lab/harbor/selftest.py
```

To exercise a real Harbor job against the mock model, start
`python3 examples/lab/tests/mock_llm.py --port-file P`, then pass
`--ak api_base=http://127.0.0.1:$(cat P)/v1` and `-m mock/model`.
