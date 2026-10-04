"""Exercise HiveAgent end to end with no Harbor job, no docker and no network.

A fake environment runs commands in a local temp dir, and tests/mock_llm.py
plays the model. Run with Harbor's interpreter so `harbor` imports:

    $(dirname $(dirname $(readlink -f ~/.local/bin/harbor)))/bin/python selftest.py
"""

import asyncio
import json
import subprocess
import sys
import tempfile
import time
from pathlib import Path

HERE = Path(__file__).resolve().parent
sys.path.insert(0, str(HERE))

from harbor.environments.base import ExecResult  # noqa: E402
from harbor.models.agent.context import AgentContext  # noqa: E402

from hive_agent import HiveAgent  # noqa: E402


class LocalEnvironment:
    """The slice of BaseEnvironment the agent uses: `exec`."""

    def __init__(self, cwd: Path) -> None:
        self.cwd = cwd
        self.commands: list[str] = []

    async def exec(self, command, cwd=None, env=None, timeout_sec=None, user=None):
        self.commands.append(command)
        proc = await asyncio.create_subprocess_exec(
            "bash", "-c", command, cwd=self.cwd,
            stdout=asyncio.subprocess.PIPE, stderr=asyncio.subprocess.PIPE,
        )
        out, err = await proc.communicate()
        return ExecResult(stdout=out.decode(), stderr=err.decode(), return_code=proc.returncode)


async def trial(mode: str, api_base: str, work: Path, session: str = "persistent") -> None:
    logs = work / f"logs-{mode}-{session}"
    env = LocalEnvironment(work)
    agent = HiveAgent(logs_dir=logs, model_name="mock/model", mode=mode,
                      api_base=api_base, max_turns=30, seat_session=session)
    argv = agent._argv(Path("swe_hive"), logs / "instruction.txt")
    assert argv[argv.index("--seat-session") + 1] == session, argv
    assert argv[argv.index("--run-id") + 1].startswith("tb-"), argv
    assert "--memory" not in argv, argv
    context = AgentContext()
    await agent.setup(env)
    await agent.run(f"Write hi into {work}/hello.txt", env, context)
    assert context.n_input_tokens and context.n_output_tokens, context
    assert (logs / "trace.jsonl").stat().st_size > 0
    result = json.loads((logs / "result.json").read_text())
    assert result["completed"] and result["mode"] == mode, result
    if mode == "hive":
        assert result["seat_session"] == session, result
    assert env.commands, "no command reached the environment"
    print(f"{mode} ({session}): ok in={context.n_input_tokens} out={context.n_output_tokens} "
          f"commands={len(env.commands)}")


def main() -> None:
    with tempfile.TemporaryDirectory() as tmp:
        work = Path(tmp)
        port_file = work / "port"
        mock = subprocess.Popen(
            [sys.executable, str(HERE.parent / "tests" / "mock_llm.py"), "--port-file", str(port_file)],
            stderr=subprocess.DEVNULL,
        )
        try:
            for _ in range(50):
                if port_file.exists() and port_file.read_text():
                    break
                time.sleep(0.1)
            api_base = f"http://127.0.0.1:{port_file.read_text()}/v1"
            for mode in ("single", "hive"):
                asyncio.run(trial(mode, api_base, work))
            asyncio.run(trial("hive", api_base, work, session="fresh"))
        finally:
            mock.kill()


if __name__ == "__main__":
    main()
