#!/bin/bash
set -euo pipefail

repo_dir="$(cd "$(dirname "$0")/.." && pwd)"
arm_target="aarch64-apple-darwin"
intel_target="x86_64-apple-darwin"
release_dir="$repo_dir/target/release-package"
version="$(tr -d '[:space:]' < "$repo_dir/release/version")"
app_dir="$release_dir/Damon.app"
stage_dir="$(mktemp -d)"
trap 'rm -rf "$stage_dir"' EXIT

rustup target add "$arm_target" "$intel_target"
cargo build --manifest-path "$repo_dir/Cargo.toml" --locked --release --target "$arm_target"
cargo build --manifest-path "$repo_dir/Cargo.toml" --locked --release --target "$intel_target"

rm -rf "$release_dir"
mkdir -p "$app_dir/Contents/MacOS" "$app_dir/Contents/Resources"
cp "$repo_dir/macos/Info.plist" "$app_dir/Contents/Info.plist"
lipo -create \
  "$repo_dir/target/$arm_target/release/damon" \
  "$repo_dir/target/$intel_target/release/damon" \
  -output "$app_dir/Contents/Resources/damon"
swiftc -parse-as-library \
  -target arm64-apple-macosx13.0 \
  "$repo_dir/macos/DamonApp.swift" \
  -o "$release_dir/Damon-arm64" \
  -framework SwiftUI \
  -framework AppKit \
  -framework Security
swiftc -parse-as-library \
  -target x86_64-apple-macosx13.0 \
  "$repo_dir/macos/DamonApp.swift" \
  -o "$release_dir/Damon-x86_64" \
  -framework SwiftUI \
  -framework AppKit \
  -framework Security
lipo -create \
  "$release_dir/Damon-arm64" \
  "$release_dir/Damon-x86_64" \
  -output "$app_dir/Contents/MacOS/Damon"
rm "$release_dir/Damon-arm64" "$release_dir/Damon-x86_64"

codesign --force --sign - "$app_dir/Contents/Resources/damon"
codesign --force --deep --sign - "$app_dir"
codesign --verify --deep --strict "$app_dir"

cp -R "$app_dir" "$stage_dir/Damon.app"
ln -s /Applications "$stage_dir/Applications"
hdiutil create \
  -volname "Damon" \
  -srcfolder "$stage_dir" \
  -ov \
  -format UDZO \
  "$release_dir/Damon-$version-Universal.dmg"
shasum -a 256 "$release_dir/Damon-$version-Universal.dmg" > "$release_dir/Damon-$version-Universal.dmg.sha256"
