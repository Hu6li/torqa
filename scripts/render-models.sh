#!/usr/bin/env sh
# Renders every building model as the world draws it into screenshots/models/ for review
# (ADR 0009). Runs inside the dev container: scripts/dev.sh scripts/render-models.sh
# Set MODELS="house_gable_2_m chalet_2_m" to render only some.
set -eu

root="$(cd "$(dirname "$0")/.." && pwd)"
scripts/build-gdext.sh debug >/dev/null
godot --headless --path "$root/app" --import >/dev/null 2>&1 || true
OUT_DIR="$root/screenshots/models" xvfb-run -a -s "-screen 0 1600x900x24" \
    godot --path "$root/app" --rendering-driver vulkan --resolution 1280x720 \
    -s res://tools/render_models.gd
