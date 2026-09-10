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
            os.killpg(process.pid, signal.SIGTERM)
            try:
                process.wait(timeout=1)
            except subprocess.TimeoutExpired:
                os.killpg(process.pid, signal.SIGKILL)

    def run(self, script: Path, cwd: Path, on_stream: StreamCallback | None = None) -> ExecutionResult:
        started = time.monotonic()
        self._cancelled = False
        process = subprocess.Popen(
            [self.executable, "-I", str(script)], cwd=cwd, stdout=subprocess.PIPE,
            stderr=subprocess.PIPE, start_new_session=True,
        )
        self._process = process
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
                    text = chunk.decode("utf-8", errors="replace")
                    if on_stream:
                        on_stream(stream, text)
                    remaining = self.policy.max_output_bytes - len(captured[stream])
                    if remaining > 0:
                        captured[stream].extend(chunk[:remaining])
                    if len(chunk) > remaining:
                        truncated = True
                if process.poll() is not None and not selector.get_map():
                    break
            exit_code = process.wait()
        finally:
            selector.close()
            self._process = None
        return ExecutionResult(
            exit_code=exit_code, duration=time.monotonic() - started,
            stdout=captured["stdout"].decode("utf-8", errors="replace"),
            stderr=captured["stderr"].decode("utf-8", errors="replace"),
            timed_out=timed_out, cancelled=self._cancelled, output_truncated=truncated,
        )
