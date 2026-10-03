#!/usr/bin/env sh
# Regenerates the boot splash PNG (app/assets/brand/torqa-splash.png) from the brand lockup SVG.
# Runs inside the dev container; rerun after changing the logo.
set -eu

root="$(cd "$(dirname "$0")/.." && pwd)"
godot --headless --path "$root/app" -s res://tools/render_splash.gd
