import subprocess
from pathlib import Path

import pytest

from damon.tools.patch import make_patch_tool


@pytest.mark.asyncio
async def test_apply_patch_checks_and_applies(tmp_path: Path):
    subprocess.run(["git", "init"], cwd=tmp_path, check=True, capture_output=True)
    path = tmp_path / "a.txt"
    path.write_text("old\n")
    patch = """diff --git a/a.txt b/a.txt
--- a/a.txt
+++ b/a.txt
@@ -1 +1 @@
-old
+new
"""
    result = await make_patch_tool(tmp_path)(patch)
    assert result["applied"] is True
    assert path.read_text() == "new\n"
