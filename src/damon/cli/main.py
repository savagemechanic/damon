from __future__ import annotations

import argparse
import asyncio
import json
import os
import urllib.error
import urllib.request
from pathlib import Path

from damon.core.agent import Agent
from damon.core.coding import CodingAgent
from damon.models.ollama import OllamaModel
from damon.models.router import ModelCandidate, ModelRouter, ModelTier, RouterPolicy
from damon.models.providers import opencode_zen, openrouter
from damon.policy.engine import Policy
from damon.runtime.bootstrap import default_registry
from damon.runtime.executor import ToolExecutor


def _add_model_args(parser: argparse.ArgumentParser) -> None:
    parser.add_argument("--model", default=os.getenv("DAMON_MODEL", "qwen3:8b"), help="primary Ollama model")
    parser.add_argument(
        "--models",
        default=os.getenv("DAMON_MODELS"),
        help="comma-separated Ollama escalation chain, cheapest first",
    )
    parser.add_argument("--local-only", action="store_true", help="forbid non-local model routes")
    parser.add_argument("--allow-cloud", action="store_true", help="allow explicitly configured cloud routes")
    parser.add_argument("--max-spend-usd", type=float, default=float(os.getenv("DAMON_MAX_SPEND_USD", "0")))
    parser.add_argument("--zen-free-model", default=os.getenv("DAMON_ZEN_FREE_MODEL"), help="OpenCode Zen free fallback")
    parser.add_argument("--openrouter-model", default=os.getenv("DAMON_OPENROUTER_MODEL"), help="paid OpenRouter fallback")
    parser.add_argument(
        "--openrouter-input-rate",
        type=float,
        default=float(os.getenv("DAMON_OPENROUTER_INPUT_RATE", "0")),
        help="USD per million input tokens",
    )
    parser.add_argument(
        "--openrouter-output-rate",
        type=float,
        default=float(os.getenv("DAMON_OPENROUTER_OUTPUT_RATE", "0")),
        help="USD per million output tokens",
    )
    parser.add_argument(
        "--openrouter-request-ceiling",
        type=float,
        default=float(os.getenv("DAMON_OPENROUTER_REQUEST_CEILING", "0")),
        help="conservative maximum USD reserved for each paid request",
    )
    parser.add_argument("--ollama-url", default=os.getenv("OLLAMA_HOST", "http://127.0.0.1:11434"))


def _parser() -> argparse.ArgumentParser:
    parser = argparse.ArgumentParser(prog="damon", description="Local-first agentic runtime")
    parser.add_argument("--root", type=Path, default=Path.cwd(), help="workspace root")
    sub = parser.add_subparsers(dest="command", required=True)

    ask = sub.add_parser("ask", help="run the agent loop")
    ask.add_argument("request")
    _add_model_args(ask)

    code = sub.add_parser("code", help="run bounded coding task orchestration")
    code.add_argument("request")
    _add_model_args(code)
    code.add_argument("--max-attempts", type=int, default=3, help="maximum coding/repair attempts")

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


def _router(args: argparse.Namespace) -> ModelRouter:
    names = [name.strip() for name in (args.models or args.model).split(",") if name.strip()]
    candidates = [
        ModelCandidate(
            name=f"ollama:{name}",
            model=OllamaModel(model=name, base_url=args.ollama_url),
            tier=ModelTier.LOCAL_SMALL if index == 0 else ModelTier.LOCAL_LARGE,
            local=True,
        )
        for index, name in enumerate(names)
    ]
    if args.zen_free_model:
        candidates.append(
            ModelCandidate(
                name=f"zen:{args.zen_free_model}",
                model=opencode_zen(args.zen_free_model),
                tier=ModelTier.CLOUD_FREE,
                local=False,
            )
        )
    if args.openrouter_model:
        candidates.append(
            ModelCandidate(
                name=f"openrouter:{args.openrouter_model}",
                model=openrouter(args.openrouter_model),
                tier=ModelTier.CLOUD_PAID,
                local=False,
                input_usd_per_million=args.openrouter_input_rate,
                output_usd_per_million=args.openrouter_output_rate,
                request_cost_ceiling_usd=args.openrouter_request_ceiling,
            )
        )
    return ModelRouter(
        candidates,
        policy=RouterPolicy(
            local_only=args.local_only,
            allow_cloud=args.allow_cloud,
            max_spend_usd=args.max_spend_usd,
        ),
    )


async def _ask(args: argparse.Namespace) -> int:
    root = args.root.resolve()
    registry = default_registry(root)
    agent = Agent(
        _router(args),
        registry,
        ToolExecutor(registry, Policy()),
    )
    print(await agent.run(args.request))
    return 0


async def _code(args: argparse.Namespace) -> int:
    root = args.root.resolve()
    registry = default_registry(root)
    agent = CodingAgent(
        _router(args),
        registry,
        ToolExecutor(registry, Policy()),
        max_attempts=args.max_attempts,
    )
    outcome = await agent.run(args.request)
    print(json.dumps(outcome.as_dict(), indent=2, default=str))
    return 0 if outcome.verification_passed is not False else 1


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
    if args.command == "code":
        return asyncio.run(_code(args))
    return 2


if __name__ == "__main__":
    raise SystemExit(main())
