from damon.core.agent import Agent
from damon.core.types import ModelResponse, ToolCall
from damon.policy.engine import Policy
from damon.runtime.executor import ToolExecutor
from damon.tools.registry import ToolRegistry, tool


class FakeModel:
    def __init__(self):
        self.calls = 0
        self.seen_tool_result = False

    async def generate(self, messages, tools):
        self.calls += 1
        if self.calls == 1:
            return ModelResponse(tool_calls=[ToolCall("1", "echo", {"text": "hello"})])
        self.seen_tool_result = any(m.role == "tool" and "hello" in m.content for m in messages)
        return ModelResponse(content="verified")


async def test_agent_executes_tool_and_feeds_result_back():
    @tool(permission="safe")
    def echo(text: str) -> str:
        """Echo text."""
        return text

    registry = ToolRegistry()
    registry.register(echo)
    model = FakeModel()
    agent = Agent(model, registry, ToolExecutor(registry, Policy()))
    assert await agent.run("test") == "verified"
    assert model.seen_tool_result
