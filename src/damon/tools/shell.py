from __future__ import annotations

import asyncio
import shlex
from pathlib import Path

from damon.tools.registry import tool

_BLOCKED = {"sudo", "su", "rm", "rmdir", "mkfs", "diskutil", "dd", "shutdown", "reboot", "halt", "killall", "launchctl", "chmod", "chown", "ssh", "scp", "curl", "wget"}


def make_shell_tool(root: Path):
    @tool(permission="process.execute", timeout=60)
    async def run_command(command: list[str], timeout: float = 30.0) -> dict:
        """Run a non-shell command in the workspace; destructive/privileged executables are blocked."""
        if not command:
            raise ValueError("command cannot be empty")
        executable = Path(command[0]).name
        if executable in _BLOCKED:
            raise PermissionError(f"executable blocked by shell policy: {executable}")
        process = await asyncio.create_subprocess_exec(*command, cwd=root, stdout=asyncio.subprocess.PIPE, stderr=asyncio.subprocess.PIPE)
        try:
            stdout, stderr = await asyncio.wait_for(process.communicate(), timeout=timeout)
        except TimeoutError:
            process.kill()
            await process.wait()
            raise TimeoutError(f"command timed out: {shlex.join(command)}")
        return {"command": shlex.join(command), "exit_code": process.returncode, "stdout": stdout.decode(errors="replace")[-12000:], "stderr": stderr.decode(errors="replace")[-12000:]}
    return run_command
