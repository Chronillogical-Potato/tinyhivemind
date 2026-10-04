# Basic OpenHuman hive runs

`basic_hive` uses two host-created OpenHuman agents. They start ordinary
sessions, join a hive, complete private assignments with the attached native
`hivemind_complete` tool, and Bob leaves before continuing his original host
session. The offline mode uses a scripted local provider. The live mode sends
those same turns to OpenRouter.

Run from the repository root:

```sh
examples/openhuman/basic-hive/run.sh offline
OPENROUTER_API_KEY=... examples/openhuman/basic-hive/run.sh live
```

`OPENROUTER_MODEL` optionally overrides the live default,
`openai/gpt-oss-120b:nitro`. The script passes the key into the container as an
environment variable; it does not put its value in the Docker build context or
command line. A root `.dockerignore` omits local targets, worktrees, Git
metadata, and `.env` files. Both modes build the same binary in Docker and run
it as an unprivileged user with a read-only filesystem. Offline mode has no
network; live mode uses Docker's bridge network for OpenRouter.

The binary checks completed hive turns, private task visibility, revoked hive
reads after Bob leaves, and an ordinary post-hive turn. In offline mode, the
captured provider request also proves Bob's pre-hive and hive inputs remain in
the same OpenHuman session and that the permanent tools remain attached.

## Recorded runs

On 2026-10-04, both commands above exited successfully in Docker. The offline
run completed Alice's and Bob's hive episodes, denied Bob a hive read after he
left, and verified his original, hive, and later host inputs in the captured
OpenHuman request. The live run used `openai/gpt-oss-120b:nitro` through
OpenRouter, completed both hive episodes without coordinator failures, and ran
Bob's ordinary host turn after he left. The live run checks the durable episode
records and session usability; it does not capture provider request bodies.

| File | Purpose |
| --- | --- |
| `Dockerfile` | Build the pinned standalone example and its runtime image. |
| `run.sh` | Run the offline or live mode inside a bounded Docker container. |
