#!/usr/bin/env sh
set -eu
if [ "$(uname -s)" != Darwin ]; then
  printf 'Mac preview requires the native macOS SDK.\n' >&2
  exit 1
fi
npm ci
npm run tauri:build:mac-arm
