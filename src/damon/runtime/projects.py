from __future__ import annotations

import json
import shutil
import sys
import tomllib
from dataclasses import dataclass
from pathlib import Path
from typing import Literal

CheckKind = Literal["test", "lint", "typecheck", "format", "build"]


@dataclass(frozen=True, slots=True)
class VerificationCommand:
    kind: CheckKind
    argv: tuple[str, ...]
    source: str
    available: bool = True

    def as_dict(self) -> dict[str, object]:
        return {
            "kind": self.kind,
            "command": list(self.argv),
            "source": self.source,
            "available": self.available,
        }


def _available(argv: tuple[str, ...]) -> bool:
    executable = argv[0]
    if Path(executable).is_absolute():
        return Path(executable).exists()
    return shutil.which(executable) is not None


def _command(kind: CheckKind, argv: tuple[str, ...], source: str) -> VerificationCommand:
    return VerificationCommand(kind=kind, argv=argv, source=source, available=_available(argv))


def _dependency_names(pyproject: dict) -> set[str]:
    project = pyproject.get("project") or {}
    raw = list(project.get("dependencies") or [])
    for values in (project.get("optional-dependencies") or {}).values():
        raw.extend(values or [])
    names: set[str] = set()
    for item in raw:
        token = str(item).strip().split(";", 1)[0].strip()
        for separator in ("[", "<", ">", "=", "!", "~", " "):
            token = token.split(separator, 1)[0]
        if token:
            names.add(token.lower().replace("_", "-"))
    return names


def _python_checks(root: Path) -> list[VerificationCommand]:
    path = root / "pyproject.toml"
    if not path.is_file():
        return []
    try:
        with path.open("rb") as handle:
            data = tomllib.load(handle)
    except (OSError, tomllib.TOMLDecodeError):
        return []

    checks: list[VerificationCommand] = []
    tools = data.get("tool") or {}
    deps = _dependency_names(data)
    python = sys.executable

    if "pytest" in tools or "pytest" in deps:
        checks.append(_command("test", (python, "-m", "pytest", "-q"), "pyproject.toml:pytest"))
    if "ruff" in tools or "ruff" in deps:
        checks.append(_command("lint", (python, "-m", "ruff", "check", "."), "pyproject.toml:ruff"))
        checks.append(_command("format", (python, "-m", "ruff", "format", "--check", "."), "pyproject.toml:ruff"))
    if "mypy" in tools or "mypy" in deps:
        checks.append(_command("typecheck", (python, "-m", "mypy", "."), "pyproject.toml:mypy"))
    if "pyright" in tools or "pyright" in deps:
        checks.append(_command("typecheck", ("pyright",), "pyproject.toml:pyright"))
    return checks


def _node_runner(root: Path) -> str:
    if (root / "pnpm-lock.yaml").exists():
        return "pnpm"
    if (root / "yarn.lock").exists():
        return "yarn"
    if (root / "bun.lock").exists() or (root / "bun.lockb").exists():
        return "bun"
    return "npm"


def _node_argv(runner: str, script: str) -> tuple[str, ...]:
    if runner == "yarn":
        return (runner, script)
    if runner == "bun":
        return (runner, "run", script)
    return (runner, "run", script)


def _node_checks(root: Path) -> list[VerificationCommand]:
    path = root / "package.json"
    if not path.is_file():
        return []
    try:
        data = json.loads(path.read_text(encoding="utf-8"))
    except (OSError, UnicodeDecodeError, json.JSONDecodeError):
        return []
    scripts = data.get("scripts") or {}
    if not isinstance(scripts, dict):
        return []
    runner = _node_runner(root)
    mapping: tuple[tuple[CheckKind, tuple[str, ...]], ...] = (
        ("test", ("test",)),
        ("lint", ("lint",)),
        ("typecheck", ("typecheck", "type-check", "check-types")),
        ("format", ("format:check", "format-check", "check-format")),
        ("build", ("build",)),
    )
    checks: list[VerificationCommand] = []
    for kind, aliases in mapping:
        script = next((name for name in aliases if name in scripts), None)
        if script:
            checks.append(_command(kind, _node_argv(runner, script), f"package.json:scripts.{script}"))
    return checks


def _rust_checks(root: Path) -> list[VerificationCommand]:
    if not (root / "Cargo.toml").is_file():
        return []
    return [
        _command("test", ("cargo", "test", "--all-targets"), "Cargo.toml"),
        _command("format", ("cargo", "fmt", "--all", "--", "--check"), "Cargo.toml"),
        _command("lint", ("cargo", "clippy", "--all-targets", "--", "-D", "warnings"), "Cargo.toml"),
    ]


def _go_checks(root: Path) -> list[VerificationCommand]:
    if not (root / "go.mod").is_file():
        return []
    return [
        _command("test", ("go", "test", "./..."), "go.mod"),
        _command("build", ("go", "build", "./..."), "go.mod"),
    ]


def _make_checks(root: Path) -> list[VerificationCommand]:
    path = root / "Makefile"
    if not path.is_file():
        return []
    try:
        lines = path.read_text(encoding="utf-8").splitlines()
    except (OSError, UnicodeDecodeError):
        return []
    targets: set[str] = set()
    for line in lines:
        if not line or line[0].isspace() or line.startswith("#") or ":" not in line:
            continue
        name = line.split(":", 1)[0].strip()
        if name and all(char.isalnum() or char in "_-" for char in name):
            targets.add(name)
    mapping: tuple[tuple[CheckKind, str], ...] = (
        ("test", "test"),
        ("lint", "lint"),
        ("typecheck", "typecheck"),
        ("format", "format-check"),
        ("build", "build"),
    )
    return [_command(kind, ("make", target), f"Makefile:{target}") for kind, target in mapping if target in targets]


def discover_verification_commands(root: Path) -> list[VerificationCommand]:
    """Discover verification commands from repository-owned configuration only."""
    root = root.resolve()
    discovered = [
        *_python_checks(root),
        *_node_checks(root),
        *_rust_checks(root),
        *_go_checks(root),
        *_make_checks(root),
    ]
    seen: set[tuple[str, ...]] = set()
    unique: list[VerificationCommand] = []
    for item in discovered:
        if item.argv in seen:
            continue
        seen.add(item.argv)
        unique.append(item)
    return unique
