from __future__ import annotations

from dataclasses import asdict
import json
from pathlib import Path

from .zen import ModelInfo


FALLBACK_MODELS = [ModelInfo("big-pickle", "Big Pickle")]


class ModelCatalog:
    def __init__(self, cache_path: Path):
        self.cache_path = cache_path

    def load(self) -> list[ModelInfo]:
        if not self.cache_path.exists():
            return FALLBACK_MODELS.copy()
        try:
            rows = json.loads(self.cache_path.read_text())["models"]
            return [ModelInfo(row["id"], row.get("name", row["id"]), tuple(row.get("reasoning_efforts", []))) for row in rows]
        except (ValueError, KeyError, TypeError):
            return FALLBACK_MODELS.copy()

    def refresh(self, provider) -> list[ModelInfo]:
        models = provider.list_models()
        if not models:
            return self.load()
        self.cache_path.parent.mkdir(parents=True, exist_ok=True)
        self.cache_path.write_text(json.dumps({"models": [asdict(model) for model in models]}, indent=2) + "\n")
        return models
