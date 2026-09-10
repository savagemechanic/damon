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
  framework_prefix="$(python3 -c "import sysconfig; print(sysconfig.get_config_var('PYTHONFRAMEWORKPREFIX'))")"
  runtime_version="$(python3 -c "import sysconfig; print(sysconfig.get_config_var('VERSION'))")"
  runtime_source="$framework_prefix/Python.framework"
  runtime_target="$output/Contents/Resources/Frameworks/Python.framework"
  ditto --noextattr --noqtn "$runtime_source" "$runtime_target"
  find "$runtime_target" -type l ! -exec test -e {} \; -delete
fi
codesign --force --deep --sign - "$output"
echo "$output"
