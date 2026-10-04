"""Harbor (Terminal-Bench) agent that drives the `swe_hive` binary.

The model is called by the compiled Rust binary on the HOST; the task
environment has no internet. Every shell command a seat wants to run comes back
to this class as one JSON line on the binary's stdout and is executed with the
task's ``environment.exec``; the result goes back on its stdin:

    binary -> agent   {"id": 1, "exec": "ls /app", "timeout": 120}
    agent  -> binary  {"id": 1, "stdout": "...", "exit": 0}

Agent kwargs (``--ak key=value``): ``mode`` (``hive`` | ``single``, default
``hive``), ``bin_path``, ``api_base``, ``max_turns``, ``round_width``,
``token_cap``, ``steps_per_turn``, ``cmd_timeout``, ``single_context``
(``none`` | ``mask`` | ``summarize``, default ``mask``: what the single seat does when its
prompt passes ``context_budget`` tokens, default 60000) and ``context_keep`` (tool
results ``mask`` keeps verbatim, default 8), ``hive_context`` (``mask`` |
``summarize``; unset picks by session mode), ``seat_session`` (``persistent`` |
``fresh``, default ``persistent``), ``memory`` (``none`` | ``cortex``, default
``none``), ``memory_url`` (else ``CORTEX_DB_URL``), ``memory_budget`` (default
1200) and ``run_id`` (default: a fresh id per trial, so trials never share
memory). The API key is read from ``OPENROUTER_API_KEY`` in the host
environment (or ``--ae``) and handed to the binary through its environment,
never through its arguments; so are ``CORTEX_DB_URL`` and ``CORTEX_DB_KEY``.
"""

from __future__ import annotations

import asyncio
import json
import os
import uuid
from pathlib import Path
from typing import Any

from harbor.agents.base import BaseAgent
from harbor.environments.base import BaseEnvironment
from harbor.models.agent.context import AgentContext

LAB = Path(__file__).resolve().parents[1]
DEFAULT_MODEL = "openai/gpt-oss-120b:nitro"
# A single request line may carry a large heredoc; asyncio's 64 KiB default is too small.
STREAM_LIMIT = 32 * 1024 * 1024


def find_binary(explicit: str | None) -> Path:
    """The `swe_hive` binary: kwarg, then $SWE_HIVE_BIN, then the lab's target dir."""
    for candidate in (
        explicit,
        os.environ.get("SWE_HIVE_BIN"),
        LAB / "target" / "release" / "swe_hive",
        LAB / "target" / "debug" / "swe_hive",
    ):
        if candidate and Path(candidate).is_file():
            return Path(candidate)
    raise FileNotFoundError(
        "swe_hive binary not found; build it with "
        "`cargo build --release --bin swe_hive` in examples/lab "
        "or set SWE_HIVE_BIN"
    )


def tokens_from_trace(trace: Path) -> tuple[int, int]:
    """Sum real tokens from `turn_finished` events; used when result.json is missing."""
    tokens_in = tokens_out = 0
    if trace.is_file():
        for line in trace.read_text().splitlines():
            try:
                event = json.loads(line)
            except ValueError:
                continue
            if event.get("event") == "turn_finished":
                tokens_in += int(event.get("input_tokens", 0))
                tokens_out += int(event.get("output_tokens", 0))
    return tokens_in, tokens_out


class HiveAgent(BaseAgent):
    """Runs the hive (or the single-agent baseline) against a Harbor task."""

    @staticmethod
    def name() -> str:
        return "swe-hive"

    def version(self) -> str | None:
        return "0.1.0"

    def __init__(
        self,
        *args: Any,
        mode: str = "hive",
        bin_path: str | None = None,
        api_base: str = "https://openrouter.ai/api/v1",
        max_turns: int | str = 60,
        round_width: int | str = 2,
        token_cap: int | str | None = None,
        steps_per_turn: int | str = 12,
        cmd_timeout: int | str = 180,
        single_context: str = "mask",
        context_budget: int | str = 60000,
        context_keep: int | str = 8,
        hive_context: str | None = None,
        seat_session: str = "persistent",
        memory: str = "none",
        memory_url: str | None = None,
        memory_budget: int | str = 1200,
        run_id: str | None = None,
        **kwargs: Any,
    ) -> None:
        super().__init__(*args, **kwargs)
        if mode not in ("hive", "single"):
            raise ValueError(f"mode must be 'hive' or 'single', not {mode!r}")
        if single_context not in ("none", "mask", "summarize", "mask+summarize"):
            raise ValueError(
                "single_context must be 'none', 'mask', 'summarize' or 'mask+summarize', "
                f"not {single_context!r}"
            )
        if hive_context not in (None, "", "mask", "summarize"):
            raise ValueError(f"hive_context must be 'mask' or 'summarize', not {hive_context!r}")
        if seat_session not in ("persistent", "fresh"):
            raise ValueError(f"seat_session must be 'persistent' or 'fresh', not {seat_session!r}")
        if memory not in ("none", "cortex"):
            raise ValueError(f"memory must be 'none' or 'cortex', not {memory!r}")
        self.hive_context = hive_context or None
        self.seat_session = seat_session
        self.memory = memory
        self.memory_url = memory_url or None
        self.memory_budget = int(memory_budget)
        self.run_id = run_id or f"tb-{uuid.uuid4().hex[:12]}"
        self.mode = mode
        self.single_context = single_context
        self.context_budget = int(context_budget)
        self.context_keep = int(context_keep)
        self.bin_path = bin_path
        self.api_base = api_base
        self.max_turns = int(max_turns)
        self.round_width = int(round_width)
        self.token_cap = int(token_cap) if token_cap not in (None, "") else None
        self.steps_per_turn = int(steps_per_turn)
        self.cmd_timeout = int(cmd_timeout)

    async def setup(self, environment: BaseEnvironment) -> None:
        find_binary(self.bin_path)  # fail the trial early, not mid-run
        if self.memory == "cortex":
            env = {**os.environ, **self._extra_env}
            if not env.get("CORTEX_DB_KEY"):
                raise RuntimeError("memory=cortex needs CORTEX_DB_KEY in the host environment")
            if not (self.memory_url or env.get("CORTEX_DB_URL")):
                raise RuntimeError("memory=cortex needs memory_url or CORTEX_DB_URL")

    def _argv(self, binary: Path, task_file: Path) -> list[str]:
        argv = [
            str(binary), "--mode", self.mode, "--task-file", str(task_file),
            "--stdio-rpc", "--model", self.model_name or DEFAULT_MODEL,
            "--api-base", self.api_base,
            "--trace", str(self.logs_dir / "trace.jsonl"),
            "--result", str(self.logs_dir / "result.json"),
            "--max-turns", str(self.max_turns),
            "--round-width", str(self.round_width),
            "--steps-per-turn", str(self.steps_per_turn),
            "--cmd-timeout", str(self.cmd_timeout),
            "--single-context", self.single_context,
            "--context-budget", str(self.context_budget),
            "--context-keep", str(self.context_keep),
        ]
        argv += ["--seat-session", self.seat_session, "--run-id", self.run_id]
        if self.hive_context is not None:
            argv += ["--hive-context", self.hive_context]
        if self.memory != "none":
            argv += ["--memory", self.memory, "--memory-budget", str(self.memory_budget)]
            if self.memory_url is not None:
                argv += ["--memory-url", self.memory_url]
        if self.token_cap is not None:
            argv += ["--token-cap", str(self.token_cap)]
        return argv

    async def run(
        self,
        instruction: str,
        environment: BaseEnvironment,
        context: AgentContext,
    ) -> None:
        self.logs_dir.mkdir(parents=True, exist_ok=True)
        task_file = self.logs_dir / "instruction.txt"
        task_file.write_text(instruction)
        env = {**os.environ, **self._extra_env}
        stderr = (self.logs_dir / "swe_hive.stderr").open("wb")
        proc = await asyncio.create_subprocess_exec(
            *self._argv(find_binary(self.bin_path), task_file),
            stdin=asyncio.subprocess.PIPE,
            stdout=asyncio.subprocess.PIPE,
            stderr=stderr,
            env=env,
            limit=STREAM_LIMIT,
        )
        try:
            await self._serve(proc, environment)
        finally:
            if proc.returncode is None:
                proc.kill()
            await proc.wait()
            stderr.close()
            self._fill_context(context)

    async def _serve(self, proc: asyncio.subprocess.Process, environment: BaseEnvironment) -> None:
        assert proc.stdin is not None and proc.stdout is not None
        write_lock = asyncio.Lock()
        jobs: set[asyncio.Task[None]] = set()

        async def reply(payload: dict[str, Any]) -> None:
            async with write_lock:
                proc.stdin.write((json.dumps(payload) + "\n").encode())
                await proc.stdin.drain()

        async def execute(request: dict[str, Any]) -> None:
            try:
                result = await environment.exec(
                    command=str(request["exec"]),
                    timeout_sec=int(request.get("timeout") or self.cmd_timeout),
                )
                out = (result.stdout or "") + (result.stderr or "")
                payload = {"id": request["id"], "stdout": out, "exit": result.return_code}
            except Exception as error:  # a failed command is the seat's problem, not ours
                payload = {"id": request["id"], "stdout": f"exec failed: {error}", "exit": -1}
            try:
                await reply(payload)
            except (BrokenPipeError, ConnectionResetError):
                pass

        while True:
            line = await proc.stdout.readline()
            if not line:
                break
            try:
                message = json.loads(line)
            except ValueError:
                continue
            if "exec" in message:
                job = asyncio.create_task(execute(message))
                jobs.add(job)
                job.add_done_callback(jobs.discard)
        if jobs:
            await asyncio.gather(*jobs, return_exceptions=True)

    def _fill_context(self, context: AgentContext) -> None:
        result_path = self.logs_dir / "result.json"
        result: dict[str, Any] = {}
        if result_path.is_file():
            try:
                result = json.loads(result_path.read_text())
            except ValueError:
                result = {}
        if result:
            tokens_in, tokens_out = int(result["tokens_in"]), int(result["tokens_out"])
        else:
            tokens_in, tokens_out = tokens_from_trace(self.logs_dir / "trace.jsonl")
        context.n_input_tokens = tokens_in
        context.n_output_tokens = tokens_out
        context.metadata = {"swe_hive": result or {"partial": True}, "mode": self.mode}
