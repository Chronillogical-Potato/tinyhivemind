# Embedded OpenHuman routing proof

This example builds one real OpenHuman `Runtime`, instantiates two independent
OpenHuman `Agent`s on it, and hands those existing handles to the first-class
`tinyhivemind-driver` binding. The same `BoundHive` binding now
backs the routing proof, the PE1006/PE1008 completion experiment, and the
DeepSWE binary; none of them maintains a second session registry.

It is deterministic and offline:

- a `SystemOneTransport` fixture returns typed Choice and Noul answers to the
  exact questions built by `JevRouter`;
- a loopback mock serves OpenHuman's incidental backend calls and its
  OpenAI-compatible model call;
- one ephemeral, read-only OpenHuman runtime owns the `engineering` and `legal`
  agents, their transcripts, session continuation, and compaction;
- `tinyhivemind-driver` validates one `HiveGraph`, binds canonical ids to
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
| `src/bin/conducted.rs` | A live completion-driven episode: the loop stepped through the `Conductor`, any of the adapter's three runners, a hidden-profile desk of five seats over OpenRouter with live Jev routing, or offline against the adapter's scripted model. `CONDUCTED_DESK=login` (default) diagnoses a regression; `CONDUCTED_DESK=triage` hands off three tickets on a budget of two, to fire the budget, the broadcast that completes its author, and the in-thread `ask` refusal. |
| `src/bin/conducted/hosted.rs` | This example as a host: `DeskJournal`, its in-memory log with the prompt and the log lines, for every runner; and `DeskHost`, an `EpisodeHost` whose seats are library sessions with the episode's belt. The runners and the loop live in `tinyhivemind-openhuman`. |
| `src/bin/conducted/jev.rs` | The live `SystemOneTransport` over `tinyjevclient`, bridged through the wire form. |
| `deepswe-sandbox/` | Reproducible local Docker image used for agent shell and test execution. |

## `conducted`: one loop, two runners

`src/bin/conducted.rs` runs one completion-driven episode through
`tinyhivemind_openhuman::run_episode`: it builds the hive, the driver, the
door and a runner, and implements `Journal` over an in-memory log. The
journal, the prompt and the log lines are the host's; the wave loop is the
adapter's, and the conversations, nudges, sorting, refusals and walls are
the conductor's, in `tinyhivemind-driver`.
How a seat's turn *runs* is behind one seam, `SeatRunner`, with three
implementations the loop cannot tell apart:

| `TINYHIVEMIND_RUNNER` | Seat | Tools | Context between turns |
| --- | --- | --- | --- |
| `embed` (default) | an `openhuman-embed` `AgentSpec` agent | the three MCP dispatchers, dialling `tinyhivemind-mcp`'s server | OpenHuman's own session, stable for the episode |
| `raw` | an `OpenHumanSessionHost` built one level down, per turn | the same four tools, in-process, each calling `EpisodeTools::call` | a per-seat log the runner keeps |
| `hosted` | the host's own seat, built once per episode through `EpisodeHost` | the same four tools, in-process, admitted over the host's gate | seeded every turn from the host's journal, up to the seat's watermark |

All three land every call in the same `EpisodeTools`, so the driver drains
identical events and a seat is refused and acknowledged in the same words
whichever runs it. The bound handle differs -- an `Agent` for embed, the raw seat
itself for raw -- which is what `tinyhivemind-driver`'s `BoundAgent` is
for: the driver stores a handle and hands it back, and never runs one.

What the raw runner establishes, and what it cost, is in its module docs. The
one thing worth knowing before reading them: a raw session still runs its turn
as a hosted root invocation, which resolves the seat against OpenHuman's
process registry and takes the model's allowlist from the seat's definition.
So the raw runner registers each seat as a workspace definition with its belt
declared by name before anything boots. A wildcard scope projects to no
declared names, and the host fails closed on an undeclared belt: the session
holds four tools and the loop sees none.

Live, with either runner:

```sh
set -a; . ~/.config/tinyhivemind/live.env; set +a
TINYHIVEMIND_LIVE_OPENROUTER=1 cargo run --manifest-path examples/openhuman/Cargo.toml --bin conducted
TINYHIVEMIND_LIVE_OPENROUTER=1 TINYHIVEMIND_RUNNER=raw cargo run --manifest-path examples/openhuman/Cargo.toml --bin conducted
```

Offline, either runner is a proof of its mechanics and needs no credential.
A scripted model answers every seat with one `complete_episode` call in
whichever dialect the request offers -- native for a raw session, or
`mcp_call_tool` against the `episode` server for an embed agent -- routing is
the deterministic fallback, and the run asserts that the call became a desk
row.

```sh
cargo run --manifest-path examples/openhuman/Cargo.toml --bin conducted
TINYHIVEMIND_RUNNER=raw cargo run --manifest-path examples/openhuman/Cargo.toml --bin conducted
TINYHIVEMIND_RUNNER=hosted cargo run --manifest-path examples/openhuman/Cargo.toml --bin conducted
```

### Benchmarking the runners

`CONDUCTED_BENCH=N` runs every runner offline, `N` episodes each on the
selected desk, and prints one table. The model is scripted, so nothing in it
is about answers: every seat completes on its first turn, and what differs
between the arms is the host. The arms share one process, so each begins
with one episode that is run and not counted, for page faults and a cold
allocator; `TINYHIVEMIND_RUNNER` names the arm that goes first (`embed` by
default, `raw` or `hosted` for the native ones), and a difference that
survives every order is the harness's.

| Column | What it is |
| --- | --- |
| `turns/ep`, `waves/ep` | seat turns the loop ran, and rounds it took |
| `requests/turn` | model calls per turn, including any discovery an embed agent spends on `mcp_list_servers` and `mcp_list_tools` |
| `KiB/turn` | request bytes sent to the model per turn: the session's history plus the delta for embed, the seeded log plus the delta for raw |
| `tool rtt ms` | from the model emitting a tool call to seeing its receipt: the whole harness in between, native or over the wire |
| `wall ms/ep` | one episode end to end |

```sh
CONDUCTED_BENCH=5 cargo run --release --manifest-path examples/openhuman/Cargo.toml --bin conducted
TINYHIVEMIND_RUNNER=raw CONDUCTED_BENCH=5 cargo run --release --manifest-path examples/openhuman/Cargo.toml --bin conducted
```

Ten episodes per arm, either order, on one laptop: `KiB/turn` 56.7 embed
against 24.4 raw, `wall ms/ep` about 41 against 31, and `tool rtt ms` the
same 9 for both. Offline, the wire costs a seat nothing it can measure; what
the raw arm saves is the bytes a turn sends and the session it does not keep.
Before the warm-up episode, whichever arm ran first reported twice the round
trip and wall of the other, and the earlier numbers in #65 read that as the
harness's.

Five episodes per arm with hosted added: hosted sends the same 24.4 KiB per
turn as raw, since both hand the tools over natively, with wall about 35 ms
against raw's 33 and embed's 43. Each offline episode is one turn, so hosted
seeding has nothing to read yet; what it costs on a longer episode is the
history it seeds, which the scripted model does not exercise.

The driver's own benchmark, `cargo run --release -p tinyhivemind-driver
--example bench`, measures the completion driver's policy with no agent at
all and binds plain seats; it says nothing about either runner.

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
no-new-privileges, the host process's numeric UID/GID, an `env -i` process
environment, and the checkout at `/workspace`. A read-only empty mount covers
`/workspace/.git`, including when the checkout's `.git` is a worktree pointer,
so agent commands cannot reach or mutate the source history. The file tools
also reject targets whose resolved path leaves `/workspace`, including through
a symlink. Model-supplied file content is limited to exactly
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
