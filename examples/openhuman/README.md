# Embedded OpenHuman routing proof

This example builds one real OpenHuman `Runtime`, instantiates two independent
OpenHuman `Agent`s on it, hands those existing handles to TinyHiveMind, and
then resolves accepted routes back to those same instances.

It is deterministic and offline:

- a `SystemOneTransport` fixture returns typed Choice and Noul answers to the
  exact questions built by `JevRouter`;
- a loopback mock serves OpenHuman's incidental backend calls and its
  OpenAI-compatible model call;
- one ephemeral, read-only OpenHuman runtime owns the `engineering` and `legal`
  agents, their transcripts, session continuation, and compaction;
- TinyHiveMind's generic `AgentRegistry<openhuman_embed::Agent>` binds route ids
  to those instances without constructing agents or storing session state;
- the engineering agent handles a routed desk turn and a deterministic DM turn
  on the same OpenHuman session, while the DM makes no System One call;
- the second provider request preserves every message from the first request as
  an exact prefix, maximizing the portion eligible for provider prompt caching;
- no credential, network provider, inherited workspace, or user data is used.

Run the standalone example from the repository root:

```sh
cargo run --manifest-path examples/openhuman/Cargo.toml
```

Expected output includes both instantiated agents, the `engineering` desk and
direct routes, exactly one System One request, two turns on one OpenHuman
session, a complete cacheable message prefix, and the mock reply
`openhuman-seat-ok`.

This proves integration mechanics, not Jev routing quality or provider
performance. Live TypeSafe quality remains the job of the labeled routing
corpus and paid campaign described in
[`docs/specs/jev-first-routing.md`](../../docs/specs/jev-first-routing.md).

## Files

| File | Purpose |
| --- | --- |
| `Cargo.toml` | Standalone dependency boundary, outside the library workspace and MSRV contract. |
| `src/main.rs` | OpenHuman runtime/agent construction, route binding, two-surface session proof, and assertions. |
| `src/bin/pe1006_hive.rs` | OpenRouter GPT-OSS completion-driven hive with stable OpenHuman sessions and live TypeSafe routing. |

Run the live hive experiment through OpenRouter:

```sh
cargo run --release --manifest-path examples/openhuman/Cargo.toml --bin pe1006_hive
```

The run requires `OPENROUTER_API_KEY`, authenticated `gh` access for one
research source, and a machine OpenHuman configuration whose memory driver is
`tinycortex`. It uses model id `openai/gpt-oss-120b:nitro` unconditionally and
writes into one durable shared workspace. By default that workspace is
`examples/openhuman/workspace/pe1006`; set `OPENHUMAN_HIVE_WORKSPACE` to use a
different directory.

The runner creates these files without overwriting existing agent edits:

```text
AGENTS.md                 shared working agreement and role boundaries
MEMORY.md                 durable, evidence-linked agent learnings
TASK.md                   official task statement
research_sources/         mirrored public research inputs
runs/run-<pid>/            one attributed transcript and OpenHuman runtime
  turns/README.md          index of every agent turn and stable session id
  turns/NNN-agent/         exact prompt, reply, and JSON metadata snapshot
```

All five agents use the workspace root as their `action_dir`, can read and
write shared files, and are instructed to update `MEMORY.md` only with
reproduced findings. Per-run OpenHuman/TinyCortex state remains isolated under
that run's directory, while the explicit workspace memory survives. The turn
snapshots record application-level prompts and final replies; OpenHuman's raw
session data and tool events remain under the same run's `openhuman-runtime/`
tree.

The live runner exposes `broadcast` and `complete_episode` through a local MCP
server. A broadcast receives a fresh TypeSafe Choice over eligible teammates;
the Choice maximum and any option strictly above 20% are assigned. Agents stay
pending after broadcasts and finish only through explicit completion calls.

This remains an experiment: GPT-OSS produced several false checker sign-offs
whose claimed files did not exist or whose algorithms failed executable
checks. A later clean run staged a newly published public implementation,
required the checker to execute its built-in brute-force checkpoints, and
independently matched the sealed oracle. The answer and derivation remain
outside the repository; the run artifacts stay under the ignored workspace.
