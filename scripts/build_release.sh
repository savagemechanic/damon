#!/bin/bash
set -euo pipefail
root="$(cd "$(dirname "$0")/.." && pwd)"
output="$root/build/Damon.app"
swift build --package-path "$root/mac" -c release
binary="$(swift build --package-path "$root/mac" -c release --show-bin-path)/Damon"
if [[ -d "$output" && "$output" == "$root/build/Damon.app" ]]; then rm -rf "$output"; fi
mkdir -p "$output/Contents/MacOS" "$output/Contents/Resources/python"
cp "$binary" "$output/Contents/MacOS/Damon"
cp "$root/mac/Damon/Info.plist" "$output/Contents/Info.plist"
ditto --noextattr --noqtn "$root/python/src" "$output/Contents/Resources/python/src"
find "$output/Contents/Resources/python/src" -type d -name __pycache__ -prune -exec rm -rf {} +
if [[ "${DAMON_BUNDLE_PYTHON:-0}" == "1" ]]; then
  bundle_python="${DAMON_BUNDLE_PYTHON_EXECUTABLE:-python3.13}"
  runtime_version="$($bundle_python -c "import sysconfig; print(sysconfig.get_config_var('VERSION'))")"
  case "$runtime_version" in
    3.12|3.13) ;;
    *) echo "release runtime must be Python 3.12 or 3.13, got $runtime_version" >&2; exit 1 ;;
  esac
  framework_prefix="$($bundle_python -c "import sysconfig; print(sysconfig.get_config_var('PYTHONFRAMEWORKPREFIX'))")"
  runtime_source="$framework_prefix/Python.framework"
  runtime_target="$output/Contents/Resources/Frameworks/Python.framework"
  ditto --noextattr --noqtn "$runtime_source" "$runtime_target"
  find "$runtime_target" -type l ! -exec test -e {} \; -delete
fi
codesign --force --deep --sign - "$output"
echo "$output"
