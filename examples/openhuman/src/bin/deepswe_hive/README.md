# DeepSWE hive binary

| File | Purpose |
| --- | --- |
| `artifacts.rs` | Validates absent outside-checkout result targets and atomically claims fresh runtime/outbox directories. |
| `task.rs` | Validates the caller-prepared pristine Git root, exact HEAD, no gitlinks/submodules, tracked/untracked/ignored cleanliness, and canonical Git metadata layout. |
| `sandbox.rs` | Confines every workspace action behind a masked-history container and produces size- and time-bounded patches in a separate no-network inspector. |
| `sandbox/` | Holds bounded preflight, container cleanup, and test-only limit seams used by every sandbox path. |
| `mcp.rs` | Serves the local workspace and TinyHiveMind MCP tools. |
| `test.rs` | Exercises parsing, refusal paths, confinement arguments, patch capture, and output shape. |

The model provider remains in the host process. Agent file and shell tools run
only inside the configured Docker image as the host process's numeric UID/GID,
with the disposable checkout mounted at `/workspace`, its real `.git` hidden by
a read-only empty mount, and no provider credential copied into the cleared
action environment. File read, write, and edit targets are resolved inside the
container and rejected when a symlink would leave `/workspace`. The runner
does not perform destructive Git operations, but an agent can modify or delete
files inside the disposable workspace it was explicitly given.

Startup rejects alternate Git metadata stored inside the checkout, every
mode-`160000` gitlink/submodule, ignored files as well as ordinary dirty files,
and any output/runtime/transcript/outbox path that is a symlink (including a
broken symlink), already exists, or is not structurally outside the canonical checkout.
The runtime and outbox directories are claimed fresh with `create_dir` before Docker or
provider setup; the output parent itself may contain the task JSON. Linked-worktree
metadata outside the checkout is mounted only into the inspector and only
read-only. All containers have no network, 1 GiB memory, 2 CPUs, 256 PIDs,
dropped capabilities, and no-new-privileges. Every shell, test, file-read, and
file-edit readback has a 600-second host deadline and a 1 MiB combined-output
limit. File-write/edit input has an exact 1 MiB (1,048,576-byte) cap. It is
staged in a host-owned temporary file, mounted read-only at the fixed
`/tmp/deepswe-input` path, consumed by a fixed wrapper, and deleted after the
action; Docker stdin is not used. Output is piped through container-side `head -c 1048577` into a mounted
file, so the host never buffers more than the limit plus one byte; overflow is
reported as `ActionOutputTooLarge`. Inspector patch capture likewise has a
600-second host deadline and pipes through container-side `head -c 33554433`
into its mounted patch file; overflow is reported as `PatchTooLarge` at the
32 MiB public limit. A cidfile identifies every action and inspector container,
which is force-removed on every outcome under a separate two-second timeout.
A bounded follow-up inspect must prove absence; cleanup failures and timeouts
are propagated as typed errors.
Preflight version/create/inspect/start capture is independently capped at
64 KiB per stream and 10 seconds. Create receives a unique name and cidfile
before it runs; create, inspect, and start all end in bounded force-removal and
an absence check, including when create hangs after daemon-side creation.

Each authorized seat gets at most three individually timed attempts to publish
its one native TinyHiveMind MCP action. The host reads the native-action outbox
after every provider outcome, including errors and timeouts, before deciding
whether an attempt is safe to retry. One accepted action is success even when
the provider's post-tool continuation fails; the transcript records both the
accepted action and that continuation error, and that seat is not rerun. More
than one action always fails immediately.

A zero-action protocol miss or retryable provider failure may retry within the
same budget and frozen pre-round view. Every attempt uses a fresh OpenHuman
conversation; the TinyHiveMind desk is the only cross-turn transcript, so an
earlier native-action acceptance cannot satisfy a later attempt or hive round.
The outbox is cleared before the first attempt and again only after such a
proven zero-action outcome.
A timeout is ambiguous because a cancelled turn may still write late, so it
fails closed without retry even when no action was observed. Provider
retryability uses OpenHuman's structured `retryable` field when present, then a
narrow fallback for its exact empty-response error, rate-limit/overload status,
and inference transport failures. Authentication, configuration, facade, tool,
and sandbox failures do not retry. Printed JSON or prose is never treated as an
action, and the host commits the whole round only after every seat has supplied
exactly one action.

The CLI requires `--task`, `--api-base`, and an absolute `--output` path;
`--model` is optional and defaults to `openai/gpt-oss-120b:nitro`.
`OPENROUTER_API_KEY` remains host-side. The task JSON requires `instance_id`,
absolute `repo_path`, `base_commit`, `problem_statement`, and `test_command`,
with unknown fields rejected. An episode may commit at most 24 seat turns.
The result is `passed` only after episode completion, a nonempty bounded patch,
and a final sandboxed test exit code of zero. The result JSON records the
instance id, status, model, committed-turn count, patch, test exit code, and
outside-checkout transcript path.
