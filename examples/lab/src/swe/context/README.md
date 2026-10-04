# context

Pure policy over one seat's JSON message list: masking old tool results,
choosing where to cut for a summary, splicing the summary in, and keeping one
recalled-memory message right after the opening.

| File | Purpose |
| --- | --- |
| `mod.rs` | `Policy` (`none`, `mask`, `summarize`, `mask+summarize`), `Settings`, `mask_observations`, `summary_cut`, `replace_prefix`, `scaled_estimate`, `upsert_memory`, `dropped_text` |
| `test.rs` | behaviour on hand-built conversations |
