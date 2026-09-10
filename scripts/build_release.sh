#!/bin/bash
set -euo pipefail
root="$(cd "$(dirname "$0")/.." && pwd)"
output="$root/build/Damon.app"
swift build --package-path "$root/mac" -c release
binary="$(swift build --package-path "$root/mac" -c release --show-bin-path)/Damon"
mkdir -p "$output/Contents/MacOS" "$output/Contents/Resources/python"
cp "$binary" "$output/Contents/MacOS/Damon"
cp "$root/mac/Damon/Info.plist" "$output/Contents/Info.plist"
ditto "$root/python/src" "$output/Contents/Resources/python/src"
echo "$output"
