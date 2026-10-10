#!/usr/bin/env sh
# Renders the Blender models (buildings, vegetation, clouds) as the world draws them into
# screenshots/models/ for review (ADR 0009). Runs inside the dev container:
# scripts/dev.sh scripts/render-models.sh
# GROUPS="vegetation clouds" or MODELS="house_gable_2_m chalet_2_m" renders only some;
# GALLERY=docs/images/models also writes the small JPEGs of the galleries in art/README.md.
set -eu

root="$(cd "$(dirname "$0")/.." && pwd)"
if [ -n "${GALLERY:-}" ]; then
    mkdir -p "$GALLERY"
    GALLERY="$(cd "$GALLERY" && pwd)"
    export GALLERY
fi
scripts/build-gdext.sh debug >/dev/null
godot --headless --path "$root/app" --import >/dev/null 2>&1 || true
# A script that does not compile leaves Godot waiting forever: find out now.
errors="$(godot --headless --path "$root/app" --check-only -s res://tools/render_models.gd 2>&1 \
    | grep -A3 "SCRIPT ERROR\|SHADER ERROR" || true)"
if [ -n "$errors" ]; then
    echo "$errors" >&2
    exit 1
fi
OUT_DIR="$root/screenshots/models" timeout 1800 xvfb-run -a -s "-screen 0 1600x900x24" \
    godot --path "$root/app" --rendering-driver vulkan --resolution 1280x720 \
    -s res://tools/render_models.gd
