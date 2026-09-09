from __future__ import annotations

import argparse
import asyncio
import json
import os
import urllib.error
import urllib.request
from pathlib import Path

from damon.core.agent import Agent
from damon.models.ollama import OllamaModel
from damon.policy.engine import Policy
from damon.runtime.bootstrap import default_registry
from damon.runtime.executor import ToolExecutor


def _parser() -> argparse.ArgumentParser:
    parser = argparse.ArgumentParser(prog="damon", description="Local-first agentic runtime")
    parser.add_argument("--root", type=Path, default=Path.cwd(), help="workspace root")
    sub = parser.add_subparsers(dest="command", required=True)

    ask = sub.add_parser("ask", help="run the coding agent")
    ask.add_argument("request")
    ask.add_argument("--model", default=os.getenv("DAMON_MODEL", "qwen3:8b"))
    ask.add_argument("--ollama-url", default=os.getenv("OLLAMA_HOST", "http://127.0.0.1:11434"))

    sub.add_parser("tools", help="list available native tools")
    sub.add_parser("doctor", help="check local prerequisites")
    return parser


def _doctor() -> int:
    checks: dict[str, object] = {"python": os.sys.version.split()[0]}
    try:
        with urllib.request.urlopen("http://127.0.0.1:11434/api/version", timeout=2) as response:
            checks["ollama"] = json.load(response)
    except (urllib.error.URLError, TimeoutError):
        checks["ollama"] = "not reachable at 127.0.0.1:11434"
    print(json.dumps(checks, indent=2))
    return 0


async def _ask(args: argparse.Namespace) -> int:
    root = args.root.resolve()
    registry = default_registry(root)
    agent = Agent(
        OllamaModel(model=args.model, base_url=args.ollama_url),
        registry,
        ToolExecutor(registry, Policy()),
    )
    print(await agent.run(args.request))
    return 0


def main() -> int:
    args = _parser().parse_args()
    if args.command == "doctor":
        return _doctor()
    registry = default_registry(args.root.resolve())
    if args.command == "tools":
        for name in registry.names():
            spec = registry.get(name)
            print(f"{name:20} {spec.permission}")
        return 0
    if args.command == "ask":
        return asyncio.run(_ask(args))
    return 2


if __name__ == "__main__":
    raise SystemExit(main())
