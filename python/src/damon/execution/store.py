from __future__ import annotations

from datetime import datetime, timezone
import hashlib
import json
from pathlib import Path


class ScriptStore:
    def __init__(self, home: Path):
        self.home = home

    def save(self, run_id: str, turn: int, source: str, model: str) -> tuple[Path, dict]:
        run_dir = self.home / "runs" / run_id
        run_dir.mkdir(parents=True, exist_ok=True)
        path = run_dir / f"{turn:03d}.py"
        path.write_text(source)
        metadata = {
            "id": f"{run_id}-{turn:03d}", "run_id": run_id, "turn": turn,
            "source_hash": hashlib.sha256(source.encode()).hexdigest(), "model": model,
            "created_at": datetime.now(timezone.utc).isoformat(), "path": str(path),
            "exit_code": None, "duration": None, "reusable_status": "none",
        }
        metadata_path = run_dir / "metadata.json"
        history = json.loads(metadata_path.read_text()) if metadata_path.exists() else {"scripts": []}
        history["scripts"].append(metadata)
        metadata_path.write_text(json.dumps(history, indent=2) + "\n")
        return path, metadata

    def record_result(self, run_id: str, script_id: str, exit_code: int | None, duration: float) -> None:
        path = self.home / "runs" / run_id / "metadata.json"
        data = json.loads(path.read_text())
        for script in data["scripts"]:
            if script["id"] == script_id:
                script["exit_code"] = exit_code
                script["duration"] = duration
                break
        path.write_text(json.dumps(data, indent=2) + "\n")
