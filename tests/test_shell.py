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
