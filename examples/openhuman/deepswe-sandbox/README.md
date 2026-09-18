# DeepSWE sandbox image

This image supplies common local build tools. Build it explicitly before a
run; the adapter never pulls or builds an image itself.

```sh
docker build -t tinyhivemind-deepswe:local .
```

Set `DEEPSWE_DOCKER_IMAGE` to a different prebuilt image when the task needs
another toolchain. Agent-action containers always receive `--network none`, a
read-write `/workspace` bind mount, an empty read-only overlay on
`/workspace/.git`, 1 GiB memory, 2 CPUs, a 256-PID limit, dropped capabilities,
no-new-privileges, and an `env -i` process environment. The separate
no-network inspector receives the same caps and sees discovered external Git
metadata read-only only while producing the final binary patch. Inspector
patch production has a 600-second host deadline and a 32 MiB output limit. It
is piped inside the container into a mounted file capped at 32 MiB plus one
overflow byte. Shell, test, file-read, and file-edit readback share a
600-second host deadline and a 1 MiB combined stdout/stderr limit, likewise
capped inside the container at 1 MiB plus one byte. Timed-out or failed Docker
CLI processes are followed by force-removal of the cidfile-identified
container under a separate two-second deadline and a bounded absence check.
Action input is capped at exactly 1 MiB, written to a host-owned temporary file
before spawn, mounted read-only at `/tmp/deepswe-input`, and removed after the
action. Docker stdin and agent-selected input paths are not used. Repositories
containing any mode-`160000` gitlink/submodule are rejected before preflight.
