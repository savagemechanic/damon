from damon.tools.registry import ToolRegistry, tool


def test_schema_generation():
    @tool(permission="safe")
    def add(a: int, b: int = 1) -> int:
        """Add integers."""
        return a + b

    registry = ToolRegistry()
    registry.register(add)
    schema = registry.schemas()[0]["function"]
    assert schema["name"] == "add"
    assert schema["parameters"]["properties"]["a"] == {"type": "integer"}
    assert schema["parameters"]["required"] == ["a"]


def test_registry_select_is_an_execution_boundary():
    @tool(permission="safe")
    def visible() -> str:
        """Visible tool."""
        return "yes"

    @tool(permission="safe")
    def hidden() -> str:
        """Hidden tool."""
        return "no"

    registry = ToolRegistry()
    registry.register(visible)
    registry.register(hidden)
    selected = registry.select({"visible"})

    assert selected.names() == ["visible"]
    assert [item["function"]["name"] for item in selected.schemas()] == ["visible"]
    try:
        selected.get("hidden")
    except KeyError:
        pass
    else:
        raise AssertionError("hidden tool leaked into selected registry")
