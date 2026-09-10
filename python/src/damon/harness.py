from __future__ import annotations

from dataclasses import asdict
import json
from pathlib import Path
import uuid
from typing import Callable

from .events import DamonEvent
from .execution.python import PythonExecutor
from .execution.store import ScriptStore
from .protocol import FinalAnswer, PythonAction, parse_action


EventSink = Callable[[DamonEvent], None]


class Harness:
    def __init__(self, provider, model, executor: PythonExecutor, script_store: ScriptStore,
                 system_prompt: str, max_turns: int = 8, thinking_effort: str | None = None):
        if max_turns <= 0:
            raise ValueError("max_turns must be positive")
        self.provider = provider
        self.model = model
        self.executor = executor
        self.script_store = script_store
        self.system_prompt = system_prompt
        self.max_turns = max_turns
        self.thinking_effort = thinking_effort

    def run(self, request: str, cwd: Path, emit: EventSink) -> str:
        run_id = str(uuid.uuid4())
        messages = [{"role": "system", "content": self.system_prompt}, {"role": "user", "content": request}]
        emit(DamonEvent("RunStarted", run_id, {"working_directory": str(cwd)}))
        for turn in range(1, self.max_turns + 1):
            emit(DamonEvent("ModelStarted", run_id, {"turn": turn, "model": self.model.id}))
            response = []
            for kind, delta in self.provider.stream_chat(messages, self.model, self.thinking_effort):
                response.append(delta) if kind == "text" else None
                emit(DamonEvent("ReasoningDelta" if kind == "reasoning" else "TextDelta", run_id, {"delta": delta}))
            text = "".join(response)
            action = parse_action(text)
            messages.append({"role": "assistant", "content": text})
            if isinstance(action, FinalAnswer):
                emit(DamonEvent("RunFinished", run_id, {"answer": action.text, "turns": turn}))
                return action.text
            assert isinstance(action, PythonAction)
            emit(DamonEvent("PythonDetected", run_id, {"source": action.source, "turn": turn}))
            path, metadata = self.script_store.save(run_id, turn, action.source, self.model.id)
            emit(DamonEvent("ScriptSaved", run_id, {
                "path": str(path), "script_id": metadata["id"], "turn": turn,
                "source_hash": metadata["source_hash"], "model": self.model.id,
                "created_at": metadata["created_at"], "reusable_status": metadata["reusable_status"],
            }))
            emit(DamonEvent("ExecutionStarted", run_id, {"script_id": metadata["id"]}))
            result = self.executor.run(path, cwd, lambda stream, delta: emit(
                DamonEvent("StdoutDelta" if stream == "stdout" else "StderrDelta", run_id, {"delta": delta})
            ))
            self.script_store.record_result(run_id, metadata["id"], result.exit_code, result.duration)
            emit(DamonEvent("ExecutionFinished", run_id, {**asdict(result), "script_id": metadata["id"]}))
            if result.cancelled:
                emit(DamonEvent("Error", run_id, {"message": "run cancelled"}))
                return ""
            observation = json.dumps(asdict(result), ensure_ascii=False)
            messages.append({"role": "user", "content": "EXECUTION RESULT\n" + observation})
            emit(DamonEvent("TurnFinished", run_id, {"turn": turn}))
        emit(DamonEvent("Error", run_id, {"message": "maximum turns exceeded"}))
        raise RuntimeError(f"maximum turns exceeded ({self.max_turns})")
