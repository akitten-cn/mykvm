#!/usr/bin/env sh
set -eu
if [ "$(uname -s)" != Darwin ]; then
  printf 'Mac preview requires the native macOS SDK.\n' >&2
  exit 1
fi
npm ci
npm run tauri:build:mac-arm

product_name="$(node -e "const fs=require('fs');const c=JSON.parse(fs.readFileSync('src-tauri/tauri.conf.json','utf8'));process.stdout.write(c.productName)")"
version="$(node -e "const fs=require('fs');const c=JSON.parse(fs.readFileSync('src-tauri/tauri.conf.json','utf8'));process.stdout.write(c.version)")"
bundle_dir="src-tauri/target/aarch64-apple-darwin/release/bundle"
app_path="$bundle_dir/macos/$product_name.app"
dmg_dir="$bundle_dir/dmg"
dmg_path="$dmg_dir/${product_name}_${version}_aarch64.dmg"

if [ ! -d "$app_path" ]; then
  printf 'Expected app bundle was not generated: %s\n' "$app_path" >&2
  exit 1
fi

stage_dir="$(mktemp -d "${TMPDIR:-/tmp}/mykvm-dmg.XXXXXX")"
trap 'rm -rf "$stage_dir"' EXIT INT TERM
mkdir -p "$dmg_dir"
cp -R "$app_path" "$stage_dir/$product_name.app"
ln -s /Applications "$stage_dir/Applications"
rm -f "$dmg_path"
hdiutil create -quiet -ov -volname "$product_name" -srcfolder "$stage_dir" -format UDZO "$dmg_path"
printf 'Created unsigned local preview: %s\n' "$app_path"
printf 'Created unsigned local preview: %s\n' "$dmg_path"
