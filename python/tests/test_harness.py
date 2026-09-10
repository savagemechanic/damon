from pathlib import Path
import sys

import pytest

from damon.execution.python import PythonExecutor
from damon.execution.store import ScriptStore
from damon.harness import Harness
from damon.providers.zen import ModelInfo


class Provider:
    def __init__(self, responses): self.responses = iter(responses); self.messages = []
    def stream_chat(self, messages, model, thinking_effort=None):
        self.messages.append(list(messages))
        yield "text", next(self.responses)


def test_harness_executes_then_returns_result_to_model(tmp_path):
    provider = Provider(["ACTION: python\n```python\nprint('real output')\n```", "ACTION: finish\nVerified."])
    events = []
    answer = Harness(provider, ModelInfo("m", "M"), PythonExecutor(sys.executable), ScriptStore(tmp_path / ".damon"), "system").run("inspect", tmp_path, events.append)
    assert answer == "Verified."
    assert "real output" in provider.messages[1][-1]["content"]
    assert (tmp_path / ".damon/runs" / events[0].run_id / "001.py").exists()
    assert [event.type for event in events] == ["RunStarted", "ModelStarted", "TextDelta", "PythonDetected", "ScriptSaved", "ExecutionStarted", "StdoutDelta", "ExecutionFinished", "TurnFinished", "ModelStarted", "TextDelta", "RunFinished"]


def test_harness_is_bounded(tmp_path):
    provider = Provider(["ACTION: python\n```python\nprint(1)\n```"])
    harness = Harness(provider, ModelInfo("m", "M"), PythonExecutor(sys.executable), ScriptStore(tmp_path), "system", max_turns=1)
    with pytest.raises(RuntimeError, match="maximum turns"):
        harness.run("loop", tmp_path, lambda event: None)
