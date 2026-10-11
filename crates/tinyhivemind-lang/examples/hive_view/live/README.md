# live

`--live`: every seat turn is a real model call through OpenRouter. See the
"Live mode" section of [`../README.md`](../README.md) for the contract.

| File | Purpose |
| --- | --- |
| `mod.rs` | `LiveSpeaker`: a `Speaker` whose rounds are concurrent model calls, metered by `--max-calls`, logged to `calls.ndjson` |
| `provider.rs` | The transport: `curl` with the key on stdin, the one request body (no tools, plugins or web search), model refusal, usage and artifacts |
| `prompt.rs` | The system and turn prompts (persona, charter, desk, grammar, the seat's projection) and the deterministic reply parser |
