#!/bin/bash
set -euo pipefail
root="$(cd "$(dirname "$0")/.." && pwd)"
temporary="$(mktemp -d /tmp/damon-smoke.XXXXXX)"
socket_path="$temporary/damon.sock"
cleanup() { [[ -n "${daemon_pid:-}" ]] && kill "$daemon_pid" 2>/dev/null || true; [[ -n "${mock_pid:-}" ]] && kill "$mock_pid" 2>/dev/null || true; rm -rf "$temporary"; }
trap cleanup EXIT

"$root/scripts/package_release.sh" >/dev/null
ditto -x -k "$root/dist/Damon-v0.1.0-macos-arm64.zip" "$temporary/extracted"
test -x "$temporary/extracted/Damon.app/Contents/MacOS/Damon"
test -f "$temporary/extracted/Damon.app/Contents/Resources/python/src/damon/harness.py"
frameworks="$temporary/extracted/Damon.app/Contents/Resources/Frameworks"
runtime="$(find "$frameworks/Python.framework/Versions" -type f -path '*/bin/python3.*' ! -name '*-config' | head -1)"
test -x "$runtime"
DYLD_FRAMEWORK_PATH="$frameworks" "$runtime" --version
python3 "$root/scripts/mock_zen.py" --port-file "$temporary/mock-port" &
mock_pid=$!
for _ in {1..100}; do [[ -f "$temporary/mock-port" ]] && break; sleep 0.1; done
test -f "$temporary/mock-port"
mock_port="$(<"$temporary/mock-port")"
DAMON_ZEN_BASE_URL="http://127.0.0.1:$mock_port/v1" DYLD_FRAMEWORK_PATH="$frameworks" PYTHONDONTWRITEBYTECODE=1 PYTHONPATH="$temporary/extracted/Damon.app/Contents/Resources/python/src" "$runtime" -m damon.ipc.server --socket "$socket_path" --home "$temporary/home" 2>"$temporary/daemon.stderr" &
daemon_pid=$!
for _ in {1..200}; do
  [[ -S "$socket_path" ]] && break
  if ! kill -0 "$daemon_pid" 2>/dev/null; then cat "$temporary/daemon.stderr" >&2; exit 1; fi
  sleep 0.1
done
if [[ ! -S "$socket_path" ]]; then cat "$temporary/daemon.stderr" >&2; echo "daemon socket readiness timed out" >&2; exit 1; fi
SOCKET_PATH="$socket_path" WORK_PATH="$temporary" python3 - <<'PY'
import json, os, socket
def request(value):
    client = socket.socket(socket.AF_UNIX)
    client.connect(os.environ["SOCKET_PATH"])
    client.sendall(json.dumps(value).encode() + b'\n')
    client.shutdown(socket.SHUT_WR)
    result = [json.loads(line) for line in client.makefile()]
    client.close()
    return result

assert request({"type": "ping"}) == [{"type": "pong", "protocol": 1}]
assert request({"type": "configure", "api_key": "smoke-secret"}) == [{"type": "configured"}]
models = request({"type": "models"})[0]
assert models["source"] == "live" and models["models"][0]["id"] == "mock-model"
events = request({"type": "run", "message": "create smoke evidence", "model": "mock-model", "thinking_effort": "low", "working_directory": os.environ["WORK_PATH"]})
types = [event["type"] for event in events]
assert "ScriptSaved" in types and "StdoutDelta" in types and "ExecutionFinished" in types
assert types[-1] == "RunFinished"
assert events[-1]["payload"]["answer"] == "Packaged conversation complete."
assert os.path.exists(os.path.join(os.environ["WORK_PATH"], "damon-smoke-output.txt"))
PY
codesign --verify --deep --strict "$temporary/extracted/Damon.app"
echo "packaged mocked-Zen execution smoke test passed"
