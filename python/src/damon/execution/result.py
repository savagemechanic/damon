from dataclasses import dataclass


@dataclass(frozen=True)
class ExecutionResult:
    exit_code: int | None
    duration: float
    stdout: str
    stderr: str
    timed_out: bool = False
    cancelled: bool = False
    output_truncated: bool = False
    changed_files: tuple[str, ...] = ()
    change_scan_truncated: bool = False
