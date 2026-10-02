#!/usr/bin/env sh
# Renders ride screenshots with software Vulkan (lavapipe) on a virtual display, so visuals can
# be checked without a GPU. Runs inside the dev container; images go to screenshots/.
set -eu

root="$(cd "$(dirname "$0")/.." && pwd)"
mkdir -p "$root/screenshots"
"$root/scripts/build-gdext.sh" debug >/dev/null
godot --headless --path "$root/app" --import >/dev/null 2>&1 || true
SCREENSHOT_DIR="$root/screenshots" xvfb-run -a -s "-screen 0 1600x900x24" \
    godot --path "$root/app" --rendering-driver vulkan --resolution 1600x900 \
    -s res://tests/screenshots.gd
