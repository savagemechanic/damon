from __future__ import annotations

import asyncio
from dataclasses import dataclass
from pathlib import Path


@dataclass(slots=True)
class CheckResult:
    command: list[str]
    exit_code: int
    stdout: str
    stderr: str

    @property
    def passed(self) -> bool:
        return self.exit_code == 0


async def run_check(root: Path, command: list[str], timeout: float = 120.0) -> CheckResult:
    process = await asyncio.create_subprocess_exec(
        *command,
        cwd=root,
        stdout=asyncio.subprocess.PIPE,
        stderr=asyncio.subprocess.PIPE,
    )
    try:
        stdout, stderr = await asyncio.wait_for(process.communicate(), timeout=timeout)
    except TimeoutError:
        process.kill()
        await process.wait()
        raise
    return CheckResult(command, process.returncode or 0, stdout.decode(errors="replace"), stderr.decode(errors="replace"))
