# `parallel/`

Tests for [`../parallel.rs`](../parallel.rs), the scoped-thread fan-out that
spreads per-room loops across cores without changing a number.

| File | Purpose |
| --- | --- |
| `test.rs` | Results come back in input order at every job count and whatever order threads finish in; the first error in input order is reported; short slices, panicking closures, and the mutable map are covered. |
