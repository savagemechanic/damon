from __future__ import annotations

import os
from pathlib import Path
import selectors
import signal
import subprocess
import time
from typing import Callable

from .policy import ExecutionPolicy
from .result import ExecutionResult


StreamCallback = Callable[[str, str], None]
_SAFE_ENVIRONMENT = {"PATH", "HOME", "TMPDIR", "TMP", "TEMP", "LANG", "LC_ALL", "USER", "LOGNAME", "SHELL"}


def _snapshot(root: Path, limit: int) -> tuple[dict[str, tuple[int, int]], bool]:
    result: dict[str, tuple[int, int]] = {}
    skipped = {".git", ".damon", ".build", "__pycache__", "node_modules", "Library"}
    for directory, names, files in os.walk(root, onerror=lambda _: None):
        names[:] = [name for name in names if name not in skipped]
        for name in files:
            path = Path(directory) / name
            try:
                stat = path.stat()
                result[str(path.relative_to(root))] = (stat.st_size, stat.st_mtime_ns)
            except (OSError, ValueError):
                continue
            if len(result) >= limit:
                return result, True
    return result, False


class PythonExecutor:
    def __init__(self, executable: str, policy: ExecutionPolicy | None = None):
        self.executable = executable
        self.policy = policy or ExecutionPolicy()
        self._process: subprocess.Popen[bytes] | None = None
        self._cancelled = False

    def cancel(self) -> None:
        self._cancelled = True
        self._terminate_group()

    def _terminate_group(self) -> None:
        process = self._process
        if process and process.poll() is None:
            try:
                os.killpg(process.pid, signal.SIGTERM)
            except ProcessLookupError:
                return
            try:
                process.wait(timeout=1)
            except subprocess.TimeoutExpired:
                try:
                    os.killpg(process.pid, signal.SIGKILL)
                except ProcessLookupError:
                    pass

    def run(self, script: Path, cwd: Path, on_stream: StreamCallback | None = None) -> ExecutionResult:
        started = time.monotonic()
        before, scan_truncated = _snapshot(cwd, self.policy.max_scanned_files)
        process = subprocess.Popen(
            [self.executable, "-I", str(script)], cwd=cwd, stdout=subprocess.PIPE,
            stderr=subprocess.PIPE, start_new_session=True,
            env={key: value for key, value in os.environ.items() if key in _SAFE_ENVIRONMENT},
        )
        self._process = process
        if self._cancelled:
            self._terminate_group()
        selector = selectors.DefaultSelector()
        assert process.stdout and process.stderr
        selector.register(process.stdout, selectors.EVENT_READ, "stdout")
        selector.register(process.stderr, selectors.EVENT_READ, "stderr")
        captured = {"stdout": bytearray(), "stderr": bytearray()}
        truncated = False
        timed_out = False
        try:
            while selector.get_map():
                if time.monotonic() - started > self.policy.timeout:
                    timed_out = True
                    self._terminate_group()
                for key, _ in selector.select(timeout=0.05):
                    chunk = os.read(key.fileobj.fileno(), 8192)
                    if not chunk:
                        selector.unregister(key.fileobj)
                        continue
                    stream = key.data
                    remaining = self.policy.max_output_bytes - len(captured[stream])
                    if remaining > 0:
                        visible = chunk[:remaining]
                        captured[stream].extend(visible)
                        if on_stream:
                            on_stream(stream, visible.decode("utf-8", errors="replace"))
                    if len(chunk) > remaining:
                        truncated = True
                if process.poll() is not None and not selector.get_map():
                    break
            exit_code = process.wait()
        finally:
            selector.close()
            self._process = None
        after, after_truncated = _snapshot(cwd, self.policy.max_scanned_files)
        changes = [f"created:{path}" for path in after.keys() - before.keys()]
        changes += [f"deleted:{path}" for path in before.keys() - after.keys()]
        changes += [f"modified:{path}" for path in before.keys() & after.keys() if before[path] != after[path]]
        return ExecutionResult(
            exit_code=exit_code, duration=time.monotonic() - started,
            stdout=captured["stdout"].decode("utf-8", errors="replace"),
            stderr=captured["stderr"].decode("utf-8", errors="replace"),
            timed_out=timed_out, cancelled=self._cancelled, output_truncated=truncated,
            changed_files=tuple(sorted(changes)), change_scan_truncated=scan_truncated or after_truncated,
        )
