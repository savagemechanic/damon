from pathlib import Path

from damon.tools.code import make_code_tools


def _inspect(root: Path):
    return make_code_tools(root)[1]()


def test_inspect_project_returns_bounded_structural_map(tmp_path: Path):
    (tmp_path / "pyproject.toml").write_text("[project]\nname='demo'\nversion='0.1'\n")
    (tmp_path / "src" / "demo").mkdir(parents=True)
    (tmp_path / "src" / "demo" / "core.py").write_text("VALUE = 1\n")
    (tmp_path / "src" / "demo" / "util.py").write_text("VALUE = 2\n")
    (tmp_path / "tests").mkdir()
    (tmp_path / "tests" / "test_core.py").write_text("def test_x(): pass\n")
    (tmp_path / "README.md").write_text("demo\n")

    result = _inspect(tmp_path)

    assert result["file_count"] == 5
    assert result["scan_truncated"] is False
    assert result["markers"] == ["pyproject.toml", "README.md"]
    assert result["source_roots"] == ["src", "tests"]
    assert result["extensions"][0] == {"extension": ".py", "files": 3}
    assert {item["path"] for item in result["top_directories"]} >= {"src", "tests", "."}
    assert "src/demo/core.py" in result["tree_preview"]


def test_inspect_project_ignores_generated_dependency_trees(tmp_path: Path):
    (tmp_path / "src").mkdir()
    (tmp_path / "src" / "main.py").write_text("print('ok')\n")
    (tmp_path / "node_modules" / "pkg").mkdir(parents=True)
    for index in range(20):
        (tmp_path / "node_modules" / "pkg" / f"{index}.js").write_text("x")
    (tmp_path / ".venv" / "lib").mkdir(parents=True)
    (tmp_path / ".venv" / "lib" / "ignored.py").write_text("x")

    result = _inspect(tmp_path)

    assert result["file_count"] == 1
    assert result["extensions"] == [{"extension": ".py", "files": 1}]
    assert all("node_modules" not in path for path in result["tree_preview"])


def test_search_code_prunes_generated_trees_and_supports_file_path(tmp_path: Path):
    (tmp_path / "src").mkdir()
    (tmp_path / "src" / "main.py").write_text("needle = 1\n")
    (tmp_path / "node_modules" / "pkg").mkdir(parents=True)
    (tmp_path / "node_modules" / "pkg" / "index.js").write_text("needle = 2\n")
    search = make_code_tools(tmp_path)[0]

    results = search("needle")
    single = search("needle", path="src/main.py")

    assert [item["path"] for item in results] == ["src/main.py"]
    assert [item["path"] for item in single] == ["src/main.py"]


def test_search_code_does_not_follow_file_symlink_outside_workspace(tmp_path: Path):
    outside = tmp_path.parent / f"{tmp_path.name}-outside.txt"
    outside.write_text("needle secret\n")
    link = tmp_path / "linked.txt"
    try:
        link.symlink_to(outside)
    except OSError:
        return
    search = make_code_tools(tmp_path)[0]

    assert search("needle") == []
