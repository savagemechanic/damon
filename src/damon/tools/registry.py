from __future__ import annotations

import inspect
import types
from dataclasses import dataclass
from pathlib import Path
from typing import Any, Callable, Union, get_args, get_origin, get_type_hints

ToolFunction = Callable[..., Any]


def _json_type(annotation: Any) -> dict[str, Any]:
    if annotation is inspect.Signature.empty or annotation is Any:
        return {}
    origin = get_origin(annotation)
    args = get_args(annotation)
    if origin in (Union, types.UnionType):
        non_none = [arg for arg in args if arg is not type(None)]
        if len(non_none) == 1:
            return _json_type(non_none[0])
    if annotation in (str, Path):
        return {"type": "string"}
    if annotation is int:
        return {"type": "integer"}
    if annotation is float:
        return {"type": "number"}
    if annotation is bool:
        return {"type": "boolean"}
    if origin is list:
        return {"type": "array", "items": _json_type(args[0]) if args else {}}
    if origin is dict or annotation is dict:
        return {"type": "object"}
    return {}


@dataclass(slots=True)
class ToolSpec:
    name: str
    description: str
    function: ToolFunction
    permission: str
    timeout: float

    def schema(self) -> dict[str, Any]:
        signature = inspect.signature(self.function)
        hints = get_type_hints(self.function)
        properties: dict[str, Any] = {}
        required: list[str] = []
        for name, parameter in signature.parameters.items():
            annotation = hints.get(name, parameter.annotation)
            properties[name] = _json_type(annotation)
            if parameter.default is inspect.Signature.empty:
                required.append(name)
        return {"type": "function", "function": {"name": self.name, "description": self.description, "parameters": {"type": "object", "properties": properties, "required": required, "additionalProperties": False}}}


def tool(*, permission: str = "safe", timeout: float = 30.0, name: str | None = None):
    def decorate(function: ToolFunction) -> ToolFunction:
        setattr(function, "__damon_tool__", {"name": name or function.__name__, "permission": permission, "timeout": timeout})
        return function
    return decorate


class ToolRegistry:
    def __init__(self) -> None:
        self._tools: dict[str, ToolSpec] = {}

    def register(self, function: ToolFunction) -> ToolSpec:
        metadata = getattr(function, "__damon_tool__", None)
        if metadata is None:
            raise ValueError(f"{function.__name__} is not decorated with @tool")
        spec = ToolSpec(name=metadata["name"], description=(inspect.getdoc(function) or "").split("\n", 1)[0], function=function, permission=metadata["permission"], timeout=float(metadata["timeout"]))
        if spec.name in self._tools:
            raise ValueError(f"duplicate tool: {spec.name}")
        self._tools[spec.name] = spec
        return spec

    def get(self, name: str) -> ToolSpec:
        try:
            return self._tools[name]
        except KeyError as exc:
            raise KeyError(f"unknown tool: {name}") from exc

    def schemas(self) -> list[dict[str, Any]]:
        return [spec.schema() for spec in self._tools.values()]

    def names(self) -> list[str]:
        return sorted(self._tools)
