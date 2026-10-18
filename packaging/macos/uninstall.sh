#!/usr/bin/env bash
set -euo pipefail
app="/Applications/Planora.app"
if [ -x "$app/Contents/MacOS/planora" ]; then
  "$app/Contents/MacOS/planora" --unregister-helper
fi
sudo rm -rf "$app"
sudo pkgutil --forget org.planora.app >/dev/null 2>&1 || true
