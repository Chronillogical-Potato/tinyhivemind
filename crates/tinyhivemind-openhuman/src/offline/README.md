# `offline`

A scripted OpenAI-compatible model on loopback that answers every seat with
one `complete_episode` call, in whichever dialect the request advertises --
native for a raw or hosted session, by whatever name the belt advertises
the tool under, `mcp_call_tool` for an embed agent -- and a closing
sentence once it sees the receipt. `Metrics` is what it saw: request bytes,
and the time from a call to its receipt. `config()` is the runtime config an
offline run boots with and `backend()` the stub for the core's non-inference
calls. `MemoryLog`, in `log.rs`, is an in-memory journal that is a real
`SessionLog`: a desk row with `only_for` reaches its author and that seat,
and a row in a conversation reaches the conversation's two seats, which is
the rule a host's own journal follows too. Compiled in tests and under the
`offline` feature.
