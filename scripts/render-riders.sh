#!/usr/bin/env sh
# Renders the riders on their bikes as the world draws them into screenshots/riders/ for review
# (ADR 0009). Runs inside the dev container: scripts/dev.sh scripts/render-riders.sh
set -eu

root="$(cd "$(dirname "$0")/.." && pwd)"
scripts/build-gdext.sh debug >/dev/null
godot --headless --path "$root/app" --import >/dev/null 2>&1 || true
# A script that does not compile leaves Godot waiting forever: find out now.
errors="$(godot --headless --path "$root/app" --check-only -s res://tools/render_riders.gd 2>&1 \
    | grep -A3 "SCRIPT ERROR\|SHADER ERROR" || true)"
if [ -n "$errors" ]; then
    echo "$errors" >&2
    exit 1
fi
OUT_DIR="$root/screenshots/riders" timeout 900 xvfb-run -a -s "-screen 0 1600x900x24" \
    godot --path "$root/app" --rendering-driver vulkan --resolution 1280x720 \
    -s res://tools/render_riders.gd
