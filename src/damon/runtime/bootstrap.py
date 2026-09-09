from __future__ import annotations

from pathlib import Path

from damon.runtime.commands import CommandRunner
from damon.tools.code import make_code_tools
from damon.tools.filesystem import make_filesystem_tools
from damon.tools.git import make_git_tools
from damon.tools.patch import make_patch_tool
from damon.tools.registry import ToolRegistry
from damon.tools.shell import make_shell_tool
from damon.tools.verify import make_verification_tools


def default_registry(root: Path) -> ToolRegistry:
    root = root.resolve()
    runner = CommandRunner(root)
    registry = ToolRegistry()
    functions = [
        *make_filesystem_tools(root),
        make_shell_tool(root, runner),
        make_patch_tool(root, runner),
        *make_git_tools(root, runner),
        *make_code_tools(root),
        *make_verification_tools(root, runner),
    ]
    for function in functions:
        registry.register(function)
    return registry
