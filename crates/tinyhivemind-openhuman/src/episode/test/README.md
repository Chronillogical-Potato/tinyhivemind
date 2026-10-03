# `episode/test`

The loop over a scripted runner and no model.

| file | holds |
| --- | --- |
| `mod.rs` | the module doc and the submodules |
| `support.rs` | `ScriptRunner`, whose turns are scripted tool calls; `TestJournal`, which records what it was shown; `BareJournal` and `GrowingLog`; the desk, the door and the calls |
| `flow.rs` | an episode with a conversation and one that stalls, and what the journal and the runner saw of each |
| `watermark.rs` | a task on a log numbered from zero, and a row the host appends above a wave's watermark |
| `journals.rs` | a journal that keeps every default is briefed as the episode words it |
