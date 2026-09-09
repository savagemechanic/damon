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
