from pathlib import Path

import pytest

from damon.runtime.commands import CommandRunner, SecretRef, classify_command


def test_classifies_git_capabilities():
    assert classify_command(["git", "status"]).capability == "git.read"
    assert classify_command(["git", "commit", "-m", "x"]).capability == "git.write"
    push = classify_command(["git", "push", "origin", "main"])
    assert push.capability == "git.push"
    assert push.network is True
    assert push.mutates is True


@pytest.mark.asyncio
async def test_runner_confines_cwd(tmp_path: Path):
    runner = CommandRunner(tmp_path)
    with pytest.raises(PermissionError):
        await runner.run(["python3", "-c", "print('x')"], cwd=tmp_path.parent)


@pytest.mark.asyncio
async def test_runner_bounds_output(tmp_path: Path):
    runner = CommandRunner(tmp_path, max_output_chars=10)
    result = await runner.run(["python3", "-c", "print('abcdefghijklmnop')"])
    assert result.ok
    assert result.output_truncated
    assert len(result.stdout) == 10


@pytest.mark.asyncio
async def test_runner_timeout_returns_structured_result(tmp_path: Path):
    runner = CommandRunner(tmp_path)
    result = await runner.run(["python3", "-c", "import time; time.sleep(5)"], timeout=0.05)
    assert result.timed_out
    assert not result.ok


@pytest.mark.asyncio
async def test_runner_resolves_secret_refs_without_exposing_ref(tmp_path: Path):
    class Resolver:
        async def resolve(self, ref: SecretRef) -> str:
            assert ref.name == "demo.token"
            return "super-secret"

    runner = CommandRunner(tmp_path, secret_resolver=Resolver())
    result = await runner.run(
        ["python3", "-c", "import os; print(os.environ['TOKEN'])"],
        env={"TOKEN": SecretRef("demo.token")},
    )
    assert result.stdout.strip() == "[REDACTED]"
    assert "super-secret" not in result.stdout
    assert "demo.token" not in result.stdout


@pytest.mark.asyncio
async def test_runner_cancellation_terminates_process_group(tmp_path: Path):
    import asyncio

    runner = CommandRunner(tmp_path)
    task = asyncio.create_task(
        runner.run(["python3", "-c", "import time; time.sleep(30)"], timeout=60)
    )
    await asyncio.sleep(0.05)
    task.cancel()
    with pytest.raises(asyncio.CancelledError):
        await task
