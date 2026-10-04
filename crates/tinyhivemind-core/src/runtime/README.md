# `tinyhivemind` feature modules

One directory per feature area, wired together and re-exported from
[`mod.rs`](mod.rs). Each answers a different question a host needs answered
about a live session; see its own `README.md` for the how and why.

| Module | Question it answers |
| --- | --- |
| [`session`](session) | How does a turn walk a host-owned, globally sequenced log into an attributed, audience-filtered transcript? |
| [`briefing`](briefing) | What ephemeral context (teammates, coordination rules, history, threads, pins) does one viewer's turn open with? |
| [`elsewhere`](elsewhere) | What do this seat's *other* conversations hold, for the turn it is taking in this one? |
| [`sharing`](sharing) | How does a host hand an already-briefed session only what changed since its last watermark, instead of re-briefing it? |
| [`pins`](pins) | Which messages does every turn see whether or not it asked? |
| [`threads`](threads) | What live threads exist in one desk, ranked by recency, for a viewer that has been away? |
| [`recall`](recall) | What does a host memory store hand a seat's persistent session — at start, on rejoin, after compaction — and what does each activation write back? |
| [`speech`](speech) | What may a seat say, what makes a call valid, and what does exactly one accepted utterance become? |
| [`error`](error) | The one `Error`/`Result<T>` every fallible function in this crate returns. |
