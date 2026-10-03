# Live mode

Drives one episode through a real agent CLI or HTTP endpoint instead of the
simulated participants in [`../sim/`](../sim/mod.rs). See
[`../LIVE.md`](../LIVE.md) for the prompt, the scenario file format, the two
backends, and what running them actually turned up, and
[`../README.md`](../README.md#live-mode) for the harness-level summary.

| file | what it does |
| --- | --- |
| [`mod.rs`](mod.rs) | Entry point: runs one episode by spawning one process (or HTTP call) per turn. Any command that reads a prompt as its final argument and prints an answer works. |
| [`agent.rs`](agent.rs) | `LiveAgent` — the plain, non-federated seat: one process per turn, answering the shared brief with no channel to any other desk. Owns running the process and parsing its answer. |
| [`desk.rs`](desk.rs) | `LiveDeskAgent` — wraps `LiveAgent` to add the one extra move a federated member has: asking another desk a question by mentioning its channel. Everything else is unchanged from `agent.rs`. |
