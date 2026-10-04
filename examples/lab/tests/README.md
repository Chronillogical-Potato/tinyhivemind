# tests

| File | Purpose |
| --- | --- |
| `mock_llm.py` | scripted OpenAI-compatible server (stdlib only) that drives both arms to completion |
| `offline.sh` | builds `swe_hive`, starts the mock and a `--network none` container, runs single, hive `--seat-session persistent --memory none` (a woken lead resumes its session) and hive `--seat-session fresh` (every activation restarts), checking `result.json` and the `session` / `memory` marks; with `CORTEX_DB_URL` and `CORTEX_DB_KEY` set it also runs both arms with `--memory cortex`; then a 24-command single session under `none`, `mask` and `summarize` (mock `LONGSESSION`) and compares `max_prompt_tokens` |

```sh
examples/lab/tests/offline.sh
```
