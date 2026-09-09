from __future__ import annotations

from pathlib import Path

import pytest

from damon.core.coding import CodingAgent
from damon.core.types import ModelResponse, ToolCall
from damon.policy.engine import Policy
from damon.runtime.bootstrap import default_registry
from damon.runtime.executor import ToolExecutor


class RepairingModel:
    def __init__(self) -> None:
        self.runs = 0
        self.calls_in_run = 0

    def reset_run(self) -> None:
        self.runs += 1
        self.calls_in_run = 0

    async def generate(self, messages, tools):
        self.calls_in_run += 1
        tool_results = [message for message in messages if message.role == "tool"]
        if not tool_results:
            content = "bad\n" if self.runs == 1 else "good\n"
            return ModelResponse(tool_calls=[ToolCall(str(self.runs), "write_file", {"path": "value.txt", "content": content})])
        return ModelResponse(content="candidate complete")


@pytest.mark.asyncio
async def test_coding_agent_repairs_after_failed_verification(tmp_path: Path):
    (tmp_path / "value.txt").write_text("old\n")
    (tmp_path / "Makefile").write_text("test:\n\t@grep -q '^good$$' value.txt\n")
    # Git status/diff tools require a repository.
    import subprocess
    subprocess.run(["git", "init"], cwd=tmp_path, check=True, capture_output=True)
    subprocess.run(["git", "add", "."], cwd=tmp_path, check=True, capture_output=True)
    subprocess.run(
        ["git", "-c", "user.email=test@example.com", "-c", "user.name=Test", "commit", "-m", "base"],
        cwd=tmp_path,
        check=True,
        capture_output=True,
    )

    registry = default_registry(tmp_path)
    model = RepairingModel()
    agent = CodingAgent(model, registry, ToolExecutor(registry, Policy()), max_attempts=2)
    outcome = await agent.run("make value.txt satisfy the repository test")

    assert len(outcome.attempts) == 2
    assert outcome.repaired is True
    assert outcome.verification_passed is True
    assert (tmp_path / "value.txt").read_text() == "good\n"
    assert "value.txt" in outcome.attempts[-1].git_diff["stdout"]


class NoEditModel:
    def reset_run(self) -> None:
        pass

    async def generate(self, messages, tools):
        return ModelResponse(content="nothing to change")


@pytest.mark.asyncio
async def test_coding_agent_stops_when_no_checks_exist(tmp_path: Path):
    import subprocess
    subprocess.run(["git", "init"], cwd=tmp_path, check=True, capture_output=True)
    registry = default_registry(tmp_path)
    agent = CodingAgent(NoEditModel(), registry, ToolExecutor(registry, Policy()), max_attempts=3)
    outcome = await agent.run("inspect only")

    assert len(outcome.attempts) == 1
    assert outcome.verification_passed is None
    assert outcome.response == "nothing to change"


def test_coding_agent_rejects_invalid_attempt_budget(tmp_path: Path):
    registry = default_registry(tmp_path)
    with pytest.raises(ValueError, match="max_attempts"):
        CodingAgent(NoEditModel(), registry, ToolExecutor(registry, Policy()), max_attempts=0)
