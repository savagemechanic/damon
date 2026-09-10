from pathlib import Path
import sys
import threading
import time

from damon.execution.policy import ExecutionPolicy
from damon.execution.python import PythonExecutor


def script(tmp_path: Path, source: str) -> Path:
    path = tmp_path / "run.py"
    path.write_text(source)
    return path


def test_streams_stdout_stderr_and_failure(tmp_path):
    chunks = []
    result = PythonExecutor(sys.executable).run(
        script(tmp_path, "import sys\nprint('out', flush=True)\nprint('bad', file=sys.stderr, flush=True)\nraise SystemExit(3)"),
        tmp_path, lambda stream, text: chunks.append((stream, text)),
    )
    assert result.exit_code == 3
    assert result.stdout == "out\n" and result.stderr == "bad\n"
    assert {name for name, _ in chunks} == {"stdout", "stderr"}


def test_timeout_terminates_process_group(tmp_path):
    result = PythonExecutor(sys.executable, ExecutionPolicy(timeout=.1)).run(
        script(tmp_path, "import time\ntime.sleep(10)"), tmp_path)
    assert result.timed_out and result.exit_code is not None
    assert result.duration < 3


def test_cancellation(tmp_path):
    executor = PythonExecutor(sys.executable)
    results = []
    thread = threading.Thread(target=lambda: results.append(executor.run(script(tmp_path, "import time\ntime.sleep(10)"), tmp_path)))
    thread.start()
    time.sleep(.1)
    executor.cancel()
    thread.join(3)
    assert not thread.is_alive()
    assert results[0].cancelled


def test_bounds_captured_output(tmp_path):
    result = PythonExecutor(sys.executable, ExecutionPolicy(max_output_bytes=10)).run(
        script(tmp_path, "print('x' * 100)"), tmp_path)
    assert len(result.stdout.encode()) == 10 and result.output_truncated
