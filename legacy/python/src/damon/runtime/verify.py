from __future__ import annotations

from dataclasses import dataclass
from pathlib import Path

from damon.runtime.commands import CommandRunner


@dataclass(slots=True)
class CheckResult:
    command: list[str]
    exit_code: int
    stdout: str
    stderr: str
    timed_out: bool = False

    @property
    def passed(self) -> bool:
        return self.exit_code == 0 and not self.timed_out


async def run_check(
    root: Path,
    command: list[str],
    timeout: float = 120.0,
    *,
    runner: CommandRunner | None = None,
) -> CheckResult:
    result = await (runner or CommandRunner(root)).run(command, timeout=timeout)
    return CheckResult(command, result.returncode, result.stdout, result.stderr, result.timed_out)
