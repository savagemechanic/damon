#!/bin/bash
set -euo pipefail
root="$(cd "$(dirname "$0")/.." && pwd)"
temporary="$(mktemp -d /tmp/damon-smoke.XXXXXX)"
socket_path="$temporary/damon.sock"
cleanup() { [[ -n "${daemon_pid:-}" ]] && kill "$daemon_pid" 2>/dev/null || true; rm -rf "$temporary"; }
trap cleanup EXIT

"$root/scripts/package_release.sh" >/dev/null
ditto -x -k "$root/dist/Damon-v0.1.0-macos-arm64.zip" "$temporary/extracted"
test -x "$temporary/extracted/Damon.app/Contents/MacOS/Damon"
test -f "$temporary/extracted/Damon.app/Contents/Resources/python/src/damon/harness.py"
frameworks="$temporary/extracted/Damon.app/Contents/Resources/Frameworks"
runtime="$(find "$frameworks/Python.framework/Versions" -type f -path '*/bin/python3.*' ! -name '*-config' | head -1)"
test -x "$runtime"
DYLD_FRAMEWORK_PATH="$frameworks" "$runtime" --version
DYLD_FRAMEWORK_PATH="$frameworks" PYTHONDONTWRITEBYTECODE=1 PYTHONPATH="$temporary/extracted/Damon.app/Contents/Resources/python/src" "$runtime" -m damon.ipc.server --socket "$socket_path" &
daemon_pid=$!
for _ in {1..50}; do [[ -S "$socket_path" ]] && break; sleep 0.05; done
test -S "$socket_path"
SOCKET_PATH="$socket_path" python3 - <<'PY'
import json, os, socket
client = socket.socket(socket.AF_UNIX)
client.connect(os.environ["SOCKET_PATH"])
client.sendall(b'{"type":"ping"}\n')
client.shutdown(socket.SHUT_WR)
assert json.loads(client.makefile().readline()) == {"type": "pong", "protocol": 1}
PY
codesign --verify --deep --strict "$temporary/extracted/Damon.app"
echo "packaged daemon IPC smoke test passed"
