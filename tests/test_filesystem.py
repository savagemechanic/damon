from pathlib import Path

import pytest

from damon.tools.filesystem import _resolve, make_filesystem_tools


def test_workspace_path_cannot_escape(tmp_path: Path):
    with pytest.raises(ValueError):
        _resolve(tmp_path, "../outside.txt")


def test_read_and_list_tools(tmp_path: Path):
    (tmp_path / "hello.txt").write_text("hello")
    tools = {fn.__name__: fn for fn in make_filesystem_tools(tmp_path)}
    assert tools["read_file"]("hello.txt") == "hello"
    assert "hello.txt" in tools["list_files"]()


def test_list_files_prunes_generated_trees(tmp_path: Path):
    (tmp_path / "src").mkdir()
    (tmp_path / "src" / "main.py").write_text("x")
    (tmp_path / "node_modules" / "pkg").mkdir(parents=True)
    (tmp_path / "node_modules" / "pkg" / "index.js").write_text("x")
    tools = {tool.__name__: tool for tool in make_filesystem_tools(tmp_path)}

    entries = tools["list_files"]()

    assert "src" in entries
    assert "src/main.py" in entries
    assert all("node_modules" not in entry for entry in entries)
