# Decision evaluation module

`mod.rs` drives paired Jev and strict-JSON calls and aggregates measurements.
`case.rs` owns the labeled corpus, `schema.rs` builds the exact dynamic JSON
Schema for the LLM baseline, and `test.rs` covers the corpus, schema, metrics,
and curl-config escaping.
