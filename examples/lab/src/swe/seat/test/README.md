# seat/test

| File | Covers |
| --- | --- |
| `mod.rs` | module doc and wiring |
| `support.rs` | scripted and recording models, fake sandbox, the `Rig` that runs one activation on a persistent session |
| `activation.rs` | tools, speech, nudges, caps, telemetry |
| `context.rs` | each context policy within one activation |
| `session.rs` | persistence across activations, delta-only resume, compaction as the only eraser, `fresh` mode |
| `memory.rs` | recall at start / rejoin / compaction, the ledger stored at the end, a failing memory |
