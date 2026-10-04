# board

| File | Purpose |
| --- | --- |
| `mod.rs` | `Board`: commit through core, `maintain` (digest fold), `briefing` (pins, digest, live tail), `read` |
| `digester.rs` | zero-token extractive `Digester` so the fold does not add model tokens |
| `test.rs` | commit, mentions, pins, digest and briefing tests |
