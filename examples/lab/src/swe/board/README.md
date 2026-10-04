# board

| File | Purpose |
| --- | --- |
| `mod.rs` | `Board`: commit through core, `maintain` (digest fold), `briefing` / `briefing_view` (pins, digest, live tail, and the watermark it was read at), `delta` (rows after a watermark, minus the seat's own, pins only when changed), `read` |
| `digester.rs` | zero-token extractive `Digester` so the fold does not add model tokens |
| `test.rs` | commit, mentions, pins, digest, briefing and delta tests |
