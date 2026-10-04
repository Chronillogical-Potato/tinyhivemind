# hive

`mod.rs` is the scheduler: a mention-driven wake queue, rounds of at most `round_width` concurrent seats, keyword routing of broadcasts, digest checkpoints, and the converged / exhausted / idle outcomes. `test.rs` covers the queue, routing and whole scripted episodes.
