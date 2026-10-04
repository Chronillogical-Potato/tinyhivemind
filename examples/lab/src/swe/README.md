# swe

A runnable software-engineering hive and its matched single-agent baseline.
Same model, same `bash` tool, same sandbox and the same token meter on both
arms, so tokens, wall clock and pass rate are comparable. All I/O lives here
because core is pure. The `swe_hive` binary (`src/bin/swe_hive.rs`) drives it.

| Directory | Role |
| --- | --- |
| `meter/` | shared token meter and the `--token-cap` / `--max-turns` caps |
| `llm/` | chat-completions client over `curl`, tool-call parsing, retry once |
| `sandbox/` | `Exec` over `docker exec` or stdio JSON-lines RPC; command policy; truncation |
| `tools/` | tool schemas: `bash` plus core `tool_specs` rendered verbatim |
| `board/` | shared transcript through core commit, pins, digest and projection |
| `roles/` | prompts for lead, implementer, tester, reviewer and the single agent |
| `seat/` | one activation: model loop, tool dispatch, telemetry |
| `hive/` | rounds of at most `--round-width` concurrent seats |
| `single/` | the baseline arm |
| `config/` | command-line parsing |
| `run/` | picks the arm, drives it, writes the `result.json` summary |

A "turn" is one model call in both arms, which is what `--max-turns` counts.
Run it offline with `tests/offline.sh`; run it under Harbor with `harbor/`.
