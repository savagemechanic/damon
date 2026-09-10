#!/bin/bash
set -euo pipefail
root="$(cd "$(dirname "$0")/.." && pwd)"
DAMON_BUNDLE_PYTHON=1 "$root/scripts/build_release.sh"
mkdir -p "$root/dist"
archive="$root/dist/Damon-v0.1.0-macos-arm64.zip"
ditto -c -k --sequesterRsrc --keepParent "$root/build/Damon.app" "$archive"
shasum -a 256 "$archive" > "$archive.sha256"
echo "$archive"
