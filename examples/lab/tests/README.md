# tests

| File | Purpose |
| --- | --- |
| `mock_llm.py` | scripted OpenAI-compatible server (stdlib only) that drives both arms to completion |
| `offline.sh` | builds `swe_hive`, starts the mock and a `--network none` container, runs both modes and checks `result.json` and the trace, then a 24-command single session under `none`, `mask` and `summarize` (mock `LONGSESSION`) and compares `max_prompt_tokens` |

```sh
examples/lab/tests/offline.sh
```
