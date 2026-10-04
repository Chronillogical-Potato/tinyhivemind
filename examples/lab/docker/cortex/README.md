# CortexDB for hive memory

A local CortexDB server that holds the hive's memory when `swe_hive` runs with
`--memory cortex`. Seats write what they did (commands, exit codes, outcomes,
failed attempts) and recall it when their session starts and after it is
compacted. Each run writes under its own namespace root, `team:<run-id>`, so
trials never share memory.

CortexDB is a closed third-party server binary (`cortexdb/cortexdb`); the
client is tinymemory's `cortex` engine. Embeddings and enrichment are computed
server-side.

```sh
# OpenRouter for embeddings and enrichment (default)
OPENROUTER_API_KEY=... docker compose -f examples/lab/docker/cortex/compose.yml up -d
curl -fsS http://127.0.0.1:3141/v1/admin/ready

# No credential: a deterministic inference double (wiring only, weak recall)
CORTEX_INFERENCE_URL=http://mock-inference:8080/v1 CORTEX_INFERENCE_KEY=mock \
  docker compose -f examples/lab/docker/cortex/compose.yml --profile mock up -d --build

# then
export CORTEX_DB_URL=http://127.0.0.1:3141 CORTEX_DB_KEY=hive-cortex-local
```

`docker compose ... down --volumes` wipes every hive's memory.

| Variable | Default | Meaning |
|---|---|---|
| `CORTEX_DB_KEY` | `hive-cortex-local` | Bearer key the server accepts and `swe_hive` sends |
| `CORTEXDB_PORT` | `3141` | Host port, bound to 127.0.0.1 |
| `CORTEXDB_VERSION` | `v0.10.4` | Server image tag |
| `CORTEX_INFERENCE_URL` | `https://openrouter.ai/api/v1` | OpenAI-compatible endpoint for every model role |
| `CORTEX_INFERENCE_KEY` | `$OPENROUTER_API_KEY` | Key for that endpoint |
| `CORTEX_EMBEDDING_MODEL` / `_DIMS` | `openai/text-embedding-3-small` / `1536` | Must match `vector_dimensions` in `cortex.toml` |
| `CORTEX_EXTRACTION_MODEL`, `CORTEX_ENRICHMENT_MODEL`, `CORTEX_ANSWER_MODEL`, `CORTEX_VERIFIER_MODEL` | `openai/gpt-4.1-mini` | Server-side enrichment models |

The embedding width is fixed on first ingest; changing the model on an existing
volume needs `down --volumes`.

| File | Purpose |
|---|---|
| `compose.yml` | The server, and the `mock` profile's inference double |
| `cortex.toml` | Server storage and index settings (1536-dim vectors) |
| `mock_inference.py`, `mock-inference.Dockerfile` | Deterministic OpenAI-compatible double, copied from tinymemory's `integration/cortexdb` harness (GPL-3.0, same as this repository) |
