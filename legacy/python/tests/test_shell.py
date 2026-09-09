from pathlib import Path

import pytest

from damon.tools.shell import make_shell_tool


@pytest.mark.asyncio
async def test_shell_blocks_destructive_commands(tmp_path: Path):
    run = make_shell_tool(tmp_path)
    with pytest.raises(PermissionError):
        await run(["rm", "-rf", "."])


@pytest.mark.asyncio
async def test_shell_returns_exit_code(tmp_path: Path):
    run = make_shell_tool(tmp_path)
    result = await run(["python3", "-c", "print('ok')"])
    assert result["exit_code"] == 0
    assert result["stdout"].strip() == "ok"


@pytest.mark.asyncio
async def test_shell_rejects_git_write_capability(tmp_path: Path):
    run = make_shell_tool(tmp_path)
    with pytest.raises(PermissionError, match="git.push"):
        await run(["git", "push", "origin", "main"])


@pytest.mark.asyncio
async def test_shell_rejects_known_mutating_process(tmp_path: Path):
    run = make_shell_tool(tmp_path)
    with pytest.raises(PermissionError, match="mutation"):
        await run(["mkdir", "generated"])
