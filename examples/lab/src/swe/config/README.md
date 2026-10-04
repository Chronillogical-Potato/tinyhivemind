# config

`mod.rs` parses the `swe_hive` flags. The API key is read from `OPENROUTER_API_KEY` by the binary, never from a flag. `test.rs` covers parsing and rejection.
