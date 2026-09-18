# Embedded OpenHuman routing proof

This example builds one real OpenHuman `Runtime`, instantiates two independent
OpenHuman `Agent`s on it, and hands those existing handles to the first-class
`tinyhivemind-openhuman` factory. The same `OpenHumanHive` binding factory now
backs the routing proof, the PE1006/PE1008 completion experiment, and the
DeepSWE binary; none of them maintains a second session registry.

It is deterministic and offline:

- a `SystemOneTransport` fixture returns typed Choice and Noul answers to the
  exact questions built by `JevRouter`;
- a loopback mock serves OpenHuman's incidental backend calls and its
  OpenAI-compatible model call;
- one ephemeral, read-only OpenHuman runtime owns the `engineering` and `legal`
  agents, their transcripts, session continuation, and compaction;
- `tinyhivemind-openhuman` validates one `HiveGraph`, binds canonical ids to
  those instances, and resolves accepted routes without constructing agents or
  storing session state;
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
| `Cargo.toml` | Standalone experiment manifest and lockfile, including the workspace's OpenHuman adapter crate. |
| `src/main.rs` | OpenHuman runtime/agent construction, route binding, two-surface session proof, and assertions. |
| `src/bin/pe1006_hive.rs` | OpenRouter GPT-OSS completion-driven hive with stable OpenHuman sessions and live TypeSafe routing. |
| `src/bin/deepswe_hive.rs` | Hermetic four-seat software-engineering hive over a caller-prepared disposable Git checkout. |
| `deepswe-sandbox/` | Reproducible local Docker image used for agent shell and test execution. |

## Hermetic DeepSWE adapter

Build the sandbox image once (the build may download operating-system packages;
the adapter itself never pulls images or installs anything):

```sh
docker build -t tinyhivemind-deepswe:local examples/openhuman/deepswe-sandbox
```

Prepare a clean, disposable local Git checkout and a task JSON containing
`instance_id`, absolute `repo_path`, `base_commit`, `problem_statement`, and
`test_command`. Then run:

```sh
OPENROUTER_API_KEY=... cargo run --release \
  --manifest-path examples/openhuman/Cargo.toml \
  --bin deepswe_hive -- \
  --task /absolute/path/task.json \
  --api-base https://openrouter.ai/api/v1 \
  --output /absolute/path/result.json
```

`--model` defaults exactly to `openai/gpt-oss-120b:nitro`. The host process
owns OpenHuman, the provider request, sessions, transcript, and outboxes. The
four initially open seats (`lead`, `implementer`, `tester`, `reviewer`) execute
bounded same-snapshot rounds through `CompletionDriver`; broadcasts use its
deterministic per-author fallback and require no TypeSafe key.

The episode admits at most 24 committed seat turns. Each authorized seat gets
at most three provider attempts and each attempt has a 600-second deadline.
After every outcome the host reconciles the native MCP outbox before deciding
whether retry is safe. A proven zero-action protocol miss or a structured
retryable provider failure may retry in the same OpenHuman session against the
same frozen round view. A timeout is ambiguous and fails closed; one accepted
action is committed even if the provider continuation then fails; multiple
actions, authentication/configuration failures, and tool or sandbox failures
fail immediately. Printed JSON or prose never counts as an action, and a round
is committed only after every seat has produced exactly one native action.

The adapter refuses a non-absolute checkout, a path other than the canonical
Git root, a non-commit base, a HEAD different from that resolved base, or any
tracked, untracked, or ignored starting entry (including an ignored `.env`). It
also rejects every Git index entry with mode `160000`: submodules/gitlinks are
unsupported whether populated, configured, ignored, or absent on disk. It
resolves both Git's absolute directory and common directory before Docker or
the provider starts. Metadata inside the checkout is accepted only for the
standard `<repo>/.git` directory; alternate in-tree metadata such as
`git init --separate-git-dir .realgit` is rejected. External metadata for a
real linked worktree remains supported. `--output` must be absolute, and the
result, transcript, outboxes, and runtime workspace are all resolved outside
the canonical checkout before sandbox or provider setup. Every destination is
checked with `symlink_metadata`; all four targets must be absent, and existing
files, empty or nonempty directories, and broken symlinks are rejected. The
output parent may already contain the task JSON and unrelated caller files.
The runtime and outbox directories are then claimed with atomic `create_dir`
calls before the provider key is read or Docker starts, so fixed session IDs
cannot resume stale state and no preexisting outbox child can be reused.

Every agent file read/write/edit and shell/test call uses a fresh container
with `--network none`, 1 GiB memory, 2 CPUs, 256 PIDs, dropped capabilities,
no-new-privileges, an `env -i` process environment, and the checkout at
`/workspace`. A read-only empty mount covers `/workspace/.git`, including when
the checkout's `.git` is a worktree pointer, so agent commands cannot reach or
mutate the source history. Model-supplied file content is limited to exactly
1 MiB (1,048,576 bytes), staged before Docker starts in a host-owned temporary
file outside the checkout, and mounted read-only at `/tmp/deepswe-input`; the
fixed container wrapper consumes that path, so Docker receives no agent-chosen
FIFO or unbounded stdin stream. The temporary file is removed after the action.
Provider credentials remain host-side.

The final patch is produced by a separate no-network inspector container with
the same resource and privilege caps. Only discovered external Git metadata is
mounted, read-only, for linked worktrees. It runs `git diff --binary
--no-ext-diff --no-textconv` and appends deterministic binary no-index
additions for every untracked, nonignored file into a fixed temporary file. A
host-side 600-second deadline kills a hung Docker CLI, and a 32 MiB patch cap is
enforced before stdout is emitted or buffered; either condition fails the run.
Docker version/create/inspect/start preflight calls have a separate 10-second
host deadline and 64 KiB stdout/stderr caps. Before create, the runner assigns
both a unique container name and cidfile, so even a create CLI that hangs after
the daemon creates the container can be cleaned up. Every cidfile-identified
action/inspector container and every uniquely named preflight container is
force-removed under its own two-second deadline. The runner then
performs a second bounded inspect to prove the container is absent; removal
failure or timeout fails the run as `CleanupFailed` or `CleanupTimeout`.
The runner never clones, fetches, pulls, resets, cleans, or commits. Agent code
can still modify or delete files in the caller-supplied disposable workspace:
confinement protects paths outside that mount and the original Git history,
not the disposable workspace contents. The result is `passed` only when the
episode completes, that bounded patch is nonempty, and the final no-network
container test exits zero; an already-passing checkout with no edit fails.

Run the real Docker fixture regression after building the image:

```sh
DEEPSWE_REAL_DOCKER_TEST=1 cargo test \
  --manifest-path examples/openhuman/Cargo.toml \
  --bin deepswe_hive real_docker
```

### Recorded local acceptance

One hermetic local fixture was run end to end and passed (`1/1`) with model id
`openai/gpt-oss-120b:nitro`. The episode committed 11 seat turns, changed the
fixture answer from `wrong` to `right`, produced a nonempty patch, and finished
with test exit code `0`. Agent action containers had internet access blocked,
and the post-run container check found no residual action, inspector, or
preflight containers.

This is acceptance evidence for the adapter and its local fixture only. It is
not an official DeepSWE score, benchmark result, or claim about corpus-wide
quality. A real score requires a supplied local DeepSWE corpus and its scorer;
this runner does not download either one.

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
`CompletionDriver` supplies the pending concrete OpenHuman agents and advances
only after the example has committed each tool utterance to its transcript.

This remains an experiment: GPT-OSS produced several false checker sign-offs
whose claimed files did not exist or whose algorithms failed executable
checks. A later clean run staged a newly published public implementation,
required the checker to execute its built-in brute-force checkpoints, and
independently matched the sealed oracle. The answer and derivation remain
outside the repository; the run artifacts stay under the ignored workspace.
