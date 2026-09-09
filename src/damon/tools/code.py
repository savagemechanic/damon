from __future__ import annotations

from collections import Counter
from pathlib import Path

from damon.tools.registry import tool
from damon.tools.filesystem import _resolve, _walk


PROJECT_MARKERS = ("pyproject.toml", "package.json", "Cargo.toml", "go.mod", "Makefile", "README.md")
_SOURCE_ROOTS = ("src", "lib", "app", "tests", "test", "packages", "crates", "cmd", "internal")


def _project_map(root: Path, *, max_files: int = 5000, preview_entries: int = 80) -> dict:
    extension_counts: Counter[str] = Counter()
    top_directory_counts: Counter[str] = Counter()
    preview: list[str] = []
    file_count = 0
    truncated = False

    for current_path, _, files in _walk(root):
        for name in sorted(files):
            path = current_path / name
            try:
                relative = path.relative_to(root)
            except ValueError:
                continue
            file_count += 1
            suffix = path.suffix.lower() or "[none]"
            extension_counts[suffix] += 1
            if len(relative.parts) > 1:
                top_directory_counts[relative.parts[0]] += 1
            else:
                top_directory_counts["."] += 1
            if len(preview) < preview_entries and len(relative.parts) <= 3:
                preview.append(str(relative))
            if file_count >= max_files:
                truncated = True
                break
        if truncated:
            break

    return {
        "file_count": file_count,
        "scan_truncated": truncated,
        "extensions": [
            {"extension": extension, "files": count}
            for extension, count in extension_counts.most_common(12)
        ],
        "top_directories": [
            {"path": path, "files": count}
            for path, count in top_directory_counts.most_common(12)
        ],
        "source_roots": [name for name in _SOURCE_ROOTS if (root / name).is_dir()],
        "tree_preview": preview,
    }


def make_code_tools(root: Path):
    @tool(permission="filesystem.read")
    def search_code(query: str, path: str = ".", max_results: int = 50) -> list[dict]:
        """Search text files for an exact string and return compact line matches."""
        base = _resolve(root, path)
        results: list[dict] = []
        files: list[Path] = []
        if base.is_file():
            files = [base]
        elif base.is_dir():
            for current, _, names in _walk(base):
                files.extend(current / name for name in names)
        for file in files:
            try:
                if file.is_symlink() or file.stat().st_size > 1_000_000:
                    continue
                text = file.read_text(encoding="utf-8")
            except (UnicodeDecodeError, OSError):
                continue
            for line_no, line in enumerate(text.splitlines(), 1):
                if query in line:
                    results.append({
                        "path": str(file.relative_to(root)),
                        "line": line_no,
                        "text": line.strip()[:300],
                    })
                    if len(results) >= max_results:
                        return results
        return results

    @tool(permission="filesystem.read")
    def inspect_project() -> dict:
        """Return a compact deterministic structural map of the repository."""
        markers = [name for name in PROJECT_MARKERS if (root / name).exists()]
        top_level = sorted(p.name for p in root.iterdir() if p.name != ".git")[:100]
        return {
            "root": str(root),
            "markers": markers,
            "top_level": top_level,
            **_project_map(root),
        }

    return [search_code, inspect_project]
