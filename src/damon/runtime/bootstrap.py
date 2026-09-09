from __future__ import annotations

from pathlib import Path

from damon.tools.code import make_code_tools
from damon.tools.filesystem import make_filesystem_tools
from damon.tools.git import make_git_tools
from damon.tools.registry import ToolRegistry
from damon.tools.patch import make_patch_tool
from damon.tools.shell import make_shell_tool


def default_registry(root: Path) -> ToolRegistry:
    registry = ToolRegistry()
    functions = [
        *make_filesystem_tools(root),
        make_shell_tool(root),
        make_patch_tool(root),
        *make_git_tools(root),
        *make_code_tools(root),
    ]
    for function in functions:
        registry.register(function)
    return registry
