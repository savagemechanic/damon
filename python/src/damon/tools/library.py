from __future__ import annotations

from datetime import datetime, timezone
import hashlib
from pathlib import Path
import re
import shutil
import sqlite3
import uuid


class ToolLibrary:
    CATEGORIES = {"git", "files", "network", "system", "misc"}

    def __init__(self, home: Path, connection: sqlite3.Connection):
        self.home = home
        self.connection = connection

    def promote(self, script: Path, name: str, description: str, category: str,
                run_id: str, model: str) -> dict:
        category = category if category in self.CATEGORIES else "misc"
        safe_name = re.sub(r"[^a-zA-Z0-9_-]+", "-", name).strip("-").lower()
        if not safe_name:
            raise ValueError("tool name must contain a letter or number")
        destination = self.home / "tools" / category / f"{safe_name}.py"
        destination.parent.mkdir(parents=True, exist_ok=True)
        if destination.exists():
            raise FileExistsError(destination)
        shutil.copy2(script, destination)
        source_hash = hashlib.sha256(destination.read_bytes()).hexdigest()
        tool = {
            "id": str(uuid.uuid4()), "name": name, "description": description,
            "path": str(destination), "category": category, "source_hash": source_hash,
            "originating_run": run_id, "originating_model": model,
            "created_at": datetime.now(timezone.utc).isoformat(),
        }
        self.connection.execute(
            "INSERT INTO tools(id,name,description,path,category,source_hash,originating_run,originating_model,created_at) VALUES(:id,:name,:description,:path,:category,:source_hash,:originating_run,:originating_model,:created_at)",
            tool,
        )
        self.connection.commit()
        return tool

    def list(self) -> list[dict]:
        rows = self.connection.execute("SELECT * FROM tools ORDER BY created_at DESC").fetchall()
        return [dict(row) for row in rows]
