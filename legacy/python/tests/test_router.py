import pytest

from damon.core.agent import Agent
from damon.core.types import ModelResponse, ToolCall
from damon.models.router import ModelCandidate, ModelRouter, ModelTier, RouterPolicy
from damon.policy.engine import Policy
from damon.runtime.executor import ToolExecutor
from damon.tools.registry import ToolRegistry, tool


class StaticModel:
    def __init__(self, response: str):
        self.response = response
        self.calls = 0

    async def generate(self, messages, tools):
        self.calls += 1
        return ModelResponse(content=self.response)


class FailingModel:
    def __init__(self):
        self.calls = 0

    async def generate(self, messages, tools):
        self.calls += 1
        raise RuntimeError("provider unavailable")


async def test_router_starts_with_cheapest_candidate():
    small = StaticModel("small")
    large = StaticModel("large")
    router = ModelRouter([
        ModelCandidate("small", small, ModelTier.LOCAL_SMALL),
        ModelCandidate("large", large, ModelTier.LOCAL_LARGE),
    ])

    response = await router.generate([], [])
    assert response.content == "small"
    assert small.calls == 1
    assert large.calls == 0
    assert router.current.name == "small"


async def test_router_escalates_after_provider_failure():
    failing = FailingModel()
    fallback = StaticModel("fallback")
    router = ModelRouter([
        ModelCandidate("small", failing),
        ModelCandidate("large", fallback, ModelTier.LOCAL_LARGE),
    ])

    response = await router.generate([], [])
    assert response.content == "fallback"
    assert router.current.name == "large"
    assert router.stats.escalations == 1
    assert router.stats.generation_failures == 1


async def test_router_escalates_after_repeated_tool_failures():
    small = StaticModel("small")
    large = StaticModel("large")
    router = ModelRouter([
        ModelCandidate("small", small),
        ModelCandidate("large", large, ModelTier.LOCAL_LARGE),
    ], policy=RouterPolicy(tool_failures_before_escalation=2))

    router.record_tool_result(ok=False)
    assert router.current.name == "small"
    router.record_tool_result(ok=False)
    assert router.current.name == "large"
    assert router.stats.escalations == 1


def test_local_only_filters_cloud_candidates():
    local = StaticModel("local")
    cloud = StaticModel("cloud")
    router = ModelRouter([
        ModelCandidate("local", local, local=True),
        ModelCandidate("cloud", cloud, ModelTier.CLOUD_FREE, local=False),
    ], policy=RouterPolicy(local_only=True))

    assert [candidate.name for candidate in router.candidates] == ["local"]


async def test_agent_feedback_drives_router_escalation():
    class ToolCallingModel:
        def __init__(self, tool_name):
            self.tool_name = tool_name

        async def generate(self, messages, tools):
            tool_results = [m for m in messages if m.role == "tool"]
            if not tool_results:
                return ModelResponse(tool_calls=[ToolCall("1", self.tool_name, {})])
            return ModelResponse(content="done")

    @tool(permission="safe")
    def broken() -> str:
        """Always fail."""
        raise RuntimeError("broken")

    registry = ToolRegistry()
    registry.register(broken)
    router = ModelRouter([
        ModelCandidate("small", ToolCallingModel("broken")),
        ModelCandidate("large", StaticModel("recovered"), ModelTier.LOCAL_LARGE),
    ], policy=RouterPolicy(tool_failures_before_escalation=1))
    agent = Agent(router, registry, ToolExecutor(registry, Policy()))

    assert await agent.run("test") == "recovered"
    assert router.current.name == "large"


def test_router_rejects_empty_eligible_set():
    cloud = StaticModel("cloud")
    with pytest.raises(ValueError, match="no eligible candidates"):
        ModelRouter([
            ModelCandidate("cloud", cloud, ModelTier.CLOUD_FREE, local=False),
        ], policy=RouterPolicy(local_only=True))


def test_cloud_is_opt_in_even_when_candidate_exists():
    local = StaticModel("local")
    cloud = StaticModel("cloud")
    router = ModelRouter([
        ModelCandidate("local", local),
        ModelCandidate("cloud", cloud, ModelTier.CLOUD_FREE, local=False),
    ])
    assert [candidate.name for candidate in router.candidates] == ["local"]


def test_free_cloud_allowed_without_spend_budget():
    cloud = StaticModel("cloud")
    router = ModelRouter([
        ModelCandidate("cloud", cloud, ModelTier.CLOUD_FREE, local=False),
    ], policy=RouterPolicy(allow_cloud=True))
    assert router.current.name == "cloud"


def test_paid_cloud_requires_positive_budget():
    paid = StaticModel("paid")
    with pytest.raises(ValueError, match="no eligible candidates"):
        ModelRouter([
            ModelCandidate("paid", paid, ModelTier.CLOUD_PAID, local=False, input_usd_per_million=1.0, request_cost_ceiling_usd=0.25),
        ], policy=RouterPolicy(allow_cloud=True, max_spend_usd=0))


async def test_router_accounts_estimated_spend():
    from damon.core.types import ModelUsage

    class UsageModel:
        async def generate(self, messages, tools):
            return ModelResponse(content="ok", usage=ModelUsage(input_tokens=1_000_000, output_tokens=500_000))

    router = ModelRouter([
        ModelCandidate(
            "paid", UsageModel(), ModelTier.CLOUD_PAID, local=False,
            input_usd_per_million=1.0, output_usd_per_million=2.0, request_cost_ceiling_usd=5.0,
        ),
    ], policy=RouterPolicy(allow_cloud=True, max_spend_usd=10.0))
    await router.generate([], [])
    assert router.stats.estimated_spend_usd == pytest.approx(2.0)
    assert router.stats.input_tokens == 1_000_000
    assert router.stats.output_tokens == 500_000


async def test_paid_route_refuses_request_when_ceiling_exceeds_remaining_budget():
    from damon.core.types import ModelUsage

    class UsageModel:
        async def generate(self, messages, tools):
            return ModelResponse(content="ok", usage=ModelUsage(input_tokens=500_000))

    model = UsageModel()
    router = ModelRouter([
        ModelCandidate(
            "paid", model, ModelTier.CLOUD_PAID, local=False,
            input_usd_per_million=1.0, request_cost_ceiling_usd=0.75,
        ),
    ], policy=RouterPolicy(allow_cloud=True, max_spend_usd=1.0))
    await router.generate([], [])
    with pytest.raises(RuntimeError, match="exceeds remaining spend budget"):
        await router.generate([], [])
