# `scenario/`

Tests for [`../scenario.rs`](../scenario.rs), the file format for a real
problem a live room solves, with the private facts behind it. The shipped
scenarios live in [`../scenarios/`](../scenarios/).

| File | Purpose |
| --- | --- |
| `test.rs` | Every shipped scenario parses, records an answer that is on offer, and gives every member something of its own. |
