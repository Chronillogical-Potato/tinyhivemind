# roles

`mod.rs` holds `Role` and the prompts: the hive rules come in two forms (a persistent seat is told it continues its own conversation and not to re-run commands it already has results for; a fresh seat that its next turn starts from the desk), plus `hive_turn` (briefing) and `hive_rejoin` (delta). `test.rs` checks composition. Prompts are short on purpose: they are paid for on every call.
