import json
import sys
from pathlib import Path

import pytest

from damon.runtime.commands import CommandRunner
from damon.runtime.projects import discover_verification_commands
from damon.tools.verify import make_verification_tools


def test_discovers_python_checks_from_pyproject(tmp_path: Path):
    (tmp_path / "pyproject.toml").write_text(
        """
[project]
name = "demo"
version = "0.1.0"

[project.optional-dependencies]
dev = ["pytest>=8", "ruff>=0.6", "mypy>=1.0"]

[tool.pytest.ini_options]
testpaths = ["tests"]
""".strip()
    )
    checks = discover_verification_commands(tmp_path)
    commands = [check.argv for check in checks]
    assert (sys.executable, "-m", "pytest", "-q") in commands
    assert (sys.executable, "-m", "ruff", "check", ".") in commands
    assert (sys.executable, "-m", "mypy", ".") in commands


def test_discovers_node_scripts_and_lockfile_runner(tmp_path: Path):
    (tmp_path / "package.json").write_text(json.dumps({
        "scripts": {
            "test": "vitest run",
            "lint": "eslint .",
            "typecheck": "tsc --noEmit",
            "build": "vite build",
        }
    }))
    (tmp_path / "pnpm-lock.yaml").write_text("lockfileVersion: '9.0'\n")
    checks = discover_verification_commands(tmp_path)
    assert [check.argv for check in checks] == [
        ("pnpm", "run", "test"),
        ("pnpm", "run", "lint"),
        ("pnpm", "run", "typecheck"),
        ("pnpm", "run", "build"),
    ]


def test_discovers_rust_go_and_make_without_duplicates(tmp_path: Path):
    (tmp_path / "Cargo.toml").write_text("[package]\nname='x'\nversion='0.1.0'\n")
    (tmp_path / "go.mod").write_text("module example.com/x\n")
    (tmp_path / "Makefile").write_text("test:\n\t@echo test\nlint:\n\t@echo lint\n")
    checks = discover_verification_commands(tmp_path)
    argv = [check.argv for check in checks]
    assert ("cargo", "test", "--all-targets") in argv
    assert ("go", "test", "./...") in argv
    assert ("make", "test") in argv
    assert len(argv) == len(set(argv))


def test_invalid_manifests_fail_closed(tmp_path: Path):
    (tmp_path / "pyproject.toml").write_text("[broken")
    (tmp_path / "package.json").write_text("{")
    assert discover_verification_commands(tmp_path) == []


@pytest.mark.asyncio
async def test_verify_project_executes_discovered_checks(tmp_path: Path):
    (tmp_path / "Makefile").write_text("test:\n\t@echo verified\n")
    discover, verify = make_verification_tools(tmp_path, CommandRunner(tmp_path))
    plan = discover()
    assert any(item["command"] == ["make", "test"] for item in plan)

    report = await verify(timeout_per_check=10)
    assert report["passed"] is True
    assert report["checks_run"] == 1
    assert report["results"][0]["stdout"].strip() == "verified"


@pytest.mark.asyncio
async def test_verify_project_reports_failure(tmp_path: Path):
    (tmp_path / "Makefile").write_text("test:\n\t@false\n")
    _, verify = make_verification_tools(tmp_path, CommandRunner(tmp_path))
    report = await verify(timeout_per_check=10)
    assert report["passed"] is False
    assert report["results"][0]["exit_code"] != 0
