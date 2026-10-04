# llm

| File | Purpose |
| --- | --- |
| `mod.rs` | `Chat` transport trait, `CurlChat` (key over curl stdin, never argv), metered `Llm` with one retry |
| `wire.rs` | pure request building and response parsing, curl config escaping |
| `test.rs` | parsing, retry, caps and curl script tests with a fake transport |
