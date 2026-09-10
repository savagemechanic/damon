#!/bin/bash
set -euo pipefail
root="$(cd "$(dirname "$0")/.." && pwd)"
"$root/scripts/build_release.sh"
destination="${DAMON_INSTALL_DIR:-$HOME/Applications}/Damon.app"
mkdir -p "$(dirname "$destination")"
ditto "$root/build/Damon.app" "$destination"
echo "$destination"
