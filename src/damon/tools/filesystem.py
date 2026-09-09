from __future__ import annotations

import os
from pathlib import Path

from damon.tools.registry import tool


IGNORED_DIRS = {
    ".git", ".venv", "venv", "node_modules", "dist", "build", "target",
    "__pycache__", ".pytest_cache", ".mypy_cache", ".ruff_cache", ".tox",
}


def _resolve(root: Path, path: str) -> Path:
    root = root.resolve()
    candidate = (root / path).resolve() if not Path(path).is_absolute() else Path(path).resolve()
    if candidate != root and root not in candidate.parents:
        raise ValueError(f"path escapes workspace: {path}")
    return candidate


def _walk(target: Path):
    for current, dirs, files in os.walk(target, topdown=True, followlinks=False):
        dirs[:] = sorted(name for name in dirs if name not in IGNORED_DIRS)
        yield Path(current), dirs, sorted(files)


def make_filesystem_tools(root: Path):
    @tool(permission="filesystem.read")
    def read_file(path: str, max_chars: int = 20000) -> str:
        """Read a UTF-8 file inside the workspace."""
        target = _resolve(root, path)
        return target.read_text(encoding="utf-8")[:max_chars]

    @tool(permission="filesystem.read")
    def list_files(path: str = ".", max_entries: int = 500) -> list[str]:
        """List workspace paths recursively while pruning generated dependency/cache trees."""
        target = _resolve(root, path)
        if target.is_file():
            return [str(target.relative_to(root))]
        entries: list[str] = []
        for current, dirs, files in _walk(target):
            for name in [*dirs, *files]:
                item = current / name
                entries.append(str(item.relative_to(root)))
                if len(entries) >= max_entries:
                    return entries
        return entries

    @tool(permission="workspace.write")
    def write_file(path: str, content: str) -> str:
        """Write a UTF-8 file inside the workspace, creating parents."""
        target = _resolve(root, path)
        target.parent.mkdir(parents=True, exist_ok=True)
        target.write_text(content, encoding="utf-8")
        return str(target.relative_to(root))

    return [read_file, list_files, write_file]
