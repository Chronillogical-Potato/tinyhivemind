# config

`mod.rs` parses the `swe_hive` flags, including `--seat-session fresh|persistent`, `--hive-context mask|summarize`, `--memory none|cortex`, `--memory-url`, `--memory-budget` and `--run-id`. The API key is read from `OPENROUTER_API_KEY` and the memory key from `CORTEX_DB_KEY` by the binary, never from a flag. `test.rs` covers parsing and rejection.
