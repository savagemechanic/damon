from __future__ import annotations

from pathlib import Path

from damon.tools.registry import tool
from damon.tools.filesystem import _resolve


def make_code_tools(root: Path):
    @tool(permission="filesystem.read")
    def search_code(query: str, path: str = ".", max_results: int = 50) -> list[dict]:
        """Search text files for an exact string and return compact line matches."""
        base = _resolve(root, path)
        results: list[dict] = []
        for file in base.rglob("*"):
            if not file.is_file() or ".git" in file.parts:
                continue
            try:
                text = file.read_text(encoding="utf-8")
            except (UnicodeDecodeError, OSError):
                continue
            for line_no, line in enumerate(text.splitlines(), 1):
                if query in line:
                    results.append({"path": str(file.relative_to(root)), "line": line_no, "text": line.strip()[:300]})
                    if len(results) >= max_results:
                        return results
        return results

    @tool(permission="filesystem.read")
    def inspect_project() -> dict:
        """Return deterministic project metadata useful before modifying a repository."""
        markers = ["pyproject.toml", "package.json", "Cargo.toml", "go.mod", "Makefile", "README.md"]
        present = [name for name in markers if (root / name).exists()]
        top_level = sorted(p.name for p in root.iterdir() if p.name != ".git")[:100]
        return {"root": str(root), "markers": present, "top_level": top_level}

    return [search_code, inspect_project]
