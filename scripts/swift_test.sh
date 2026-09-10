#!/bin/bash
set -euo pipefail
cd "$(dirname "$0")/../mac"

frameworks=/Library/Developer/CommandLineTools/Library/Developer/Frameworks
interop=/Library/Developer/CommandLineTools/Library/Developer/usr/lib/lib_TestingInterop.dylib
if [[ "$(xcode-select -p)" == "/Library/Developer/CommandLineTools" && -d "$frameworks/Testing.framework" ]]; then
  swift build --build-tests -Xswiftc -F -Xswiftc "$frameworks" -Xlinker "-F$frameworks"
  debug_dir="$(swift build --show-bin-path)"
  ditto "$frameworks/Testing.framework" "$debug_dir/Testing.framework"
  cp "$interop" "$debug_dir/lib_TestingInterop.dylib"
  swift test --skip-build -Xswiftc -F -Xswiftc "$frameworks" -Xlinker "-F$frameworks"
else
  swift test
fi
