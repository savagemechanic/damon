from __future__ import annotations

import json

from damon.core.types import Message
from damon.models.base import Model
from damon.runtime.events import Event, EventBus
from damon.runtime.executor import ToolExecutor
from damon.tools.registry import ToolRegistry

DEFAULT_SYSTEM_PROMPT = """You are Damon, a local-first coding agent.
Prefer deterministic tools over guessing. Inspect before editing. Make minimal changes.
After changes, run relevant verification. Never claim success without tool evidence.
Do not commit, push, delete, or perform privileged actions unless policy permits them.
Keep tool arguments precise and outputs concise."""


class Agent:
    def __init__(
        self,
        model: Model,
        registry: ToolRegistry,
        executor: ToolExecutor,
        *,
        max_steps: int = 16,
        events: EventBus | None = None,
        system_prompt: str = DEFAULT_SYSTEM_PROMPT,
    ) -> None:
        self.model = model
        self.registry = registry
        self.executor = executor
        self.max_steps = max_steps
        self.events = events or EventBus()
        self.system_prompt = system_prompt

    async def run(self, request: str) -> str:
        reset_run = getattr(self.model, "reset_run", None)
        if callable(reset_run):
            reset_run()
        messages = [Message("system", self.system_prompt), Message("user", request)]
        for step in range(self.max_steps):
            await self.events.emit(Event.make("model.started", {"step": step}))
            response = await self.model.generate(messages, self.registry.schemas())
            assistant_calls = [
                {"type": "function", "function": {"name": call.name, "arguments": call.arguments}}
                for call in response.tool_calls
            ]
            messages.append(Message("assistant", response.content, tool_calls=assistant_calls or None))
            if not response.tool_calls:
                await self.events.emit(Event.make("agent.completed", {"steps": step + 1}))
                return response.content

            for call in response.tool_calls:
                await self.events.emit(Event.make("tool.started", {"name": call.name}))
                result = await self.executor.execute(call.name, call.arguments)
                record_tool_result = getattr(self.model, "record_tool_result", None)
                if callable(record_tool_result):
                    record_tool_result(ok=result.ok)
                payload = {"ok": result.ok, "output": result.output, "error": result.error}
                messages.append(
                    Message(
                        "tool",
                        json.dumps(payload, default=str),
                        name=call.name,
                        tool_call_id=call.id,
                        tool_name=call.name,
                    )
                )
                await self.events.emit(Event.make("tool.finished", {"name": call.name, "ok": result.ok}))
        raise RuntimeError(f"agent exceeded max_steps={self.max_steps}")
