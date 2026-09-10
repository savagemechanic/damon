from __future__ import annotations

from dataclasses import asdict
import json
from pathlib import Path
import sys
import threading
from typing import Callable

from damon.config.settings import Settings
from damon.execution.policy import ExecutionPolicy
from damon.execution.python import PythonExecutor
from damon.execution.store import ScriptStore
from damon.harness import Harness
from damon.providers.catalog import ModelCatalog
from damon.providers.zen import ZenProvider
from damon.state.sqlite import DamonStore
from damon.tools.library import ToolLibrary


Emit = Callable[[dict], None]


class DamonService:
    def __init__(self, home: Path, provider_factory=ZenProvider):
        self.home = home
        self.provider_factory = provider_factory
        self.api_key: str | None = None
        self.settings = Settings.load(home / "config.json")
        self.catalog = ModelCatalog(home / "models.json")
        self.store = DamonStore(home / "damon.sqlite3")
        self.tools = ToolLibrary(home, self.store.connection, self.store.lock)
        self._active_executors: set[PythonExecutor] = set()
        self._active_lock = threading.Lock()

    def dispatch(self, request: dict, emit: Emit) -> None:
        kind = request.get("type")
        if kind == "ping":
            emit({"type": "pong", "protocol": 1})
        elif kind == "configure":
            key = request.get("api_key", "")
            if not isinstance(key, str) or not key:
                raise ValueError("api_key is required")
            self.api_key = key
            emit({"type": "configured"})
        elif kind == "clear_api_key":
            self.api_key = None
            emit({"type": "api_key_cleared"})
        elif kind == "models":
            provider = self._provider()
            try:
                models = self.catalog.refresh(provider)
                source = "live"
            except Exception:
                models = self.catalog.load()
                source = "cache"
            emit({"type": "models", "source": source, "models": [asdict(model) for model in models]})
        elif kind == "tools":
            emit({"type": "tools", "tools": self.tools.list()})
        elif kind == "chats":
            emit({"type": "chats", "chats": self.store.list_chats()})
        elif kind == "messages":
            chat_id = request.get("chat_id")
            if not isinstance(chat_id, str) or not chat_id:
                raise ValueError("chat_id is required")
            emit({"type": "messages", "messages": self.store.chat_messages(chat_id)})
        elif kind == "promote":
            tool = self.tools.promote(
                Path(request["path"]), request["name"], request.get("description", ""),
                request.get("category", "misc"), request.get("run_id", "unknown"),
                request.get("model", "unknown"),
            )
            emit({"type": "tool_saved", "tool": tool})
        elif kind == "cancel":
            with self._active_lock:
                active = list(self._active_executors)
            for executor in active:
                executor.cancel()
            emit({"type": "cancelled", "count": len(active)})
        elif kind == "run":
            self._run(request, emit)
        else:
            raise ValueError("unknown request type")

    def _provider(self):
        if not self.api_key:
            raise RuntimeError("OpenCode Zen API key is not configured")
        return self.provider_factory(self.api_key)

    def _run(self, request: dict, emit: Emit) -> None:
        message = request.get("message", "")
        model_id = request.get("model", self.settings.model)
        if not isinstance(message, str) or not message.strip():
            raise ValueError("message is required")
        models = self.catalog.load()
        model = next((item for item in models if item.id == model_id), None)
        if model is None:
            raise ValueError("selected model is not in the cached Zen catalogue")
        effort = request.get("thinking_effort") or None
        provider = self._provider()
        executor = PythonExecutor(
            request.get("python_executable", self.settings.python_executable or sys.executable),
            ExecutionPolicy(float(request.get("execution_timeout", self.settings.execution_timeout))),
        )
        with self._active_lock:
            self._active_executors.add(executor)
        harness = Harness(
            provider, model, executor, ScriptStore(self.home),
            request.get("system_prompt", self.settings.system_prompt),
            int(request.get("max_turns", self.settings.max_turns)),
            thinking_effort=effort,
        )
        cwd = Path(request.get("working_directory", Path.home())).resolve()
        chat_id = request.get("chat_id") or self.store.create_chat(message)
        self.store.add_message(chat_id, "user", message)
        run_id: str | None = None

        def persist(event) -> None:
            nonlocal run_id
            if event.type == "RunStarted":
                run_id = event.run_id
                self.store.start_run(run_id, chat_id)
            self.store.add_event(event)
            run_dir = self.home / "runs" / event.run_id
            run_dir.mkdir(parents=True, exist_ok=True)
            with (run_dir / "events.jsonl").open("a") as stream:
                stream.write(event.to_json() + "\n")
            if event.type == "RunFinished":
                self.store.finish_run(event.run_id, "finished")
                self.store.add_message(chat_id, "assistant", event.payload["answer"])
            elif event.type == "Error":
                self.store.finish_run(event.run_id, "error")
            data = json.loads(event.to_json())
            data["chat_id"] = chat_id
            emit(data)

        try:
            harness.run(message, cwd, persist)
        except Exception:
            if run_id:
                self.store.finish_run(run_id, "error")
            raise
        finally:
            with self._active_lock:
                self._active_executors.discard(executor)
