#!/usr/bin/env sh
# Renders ride screenshots with software Vulkan (lavapipe) on a virtual display, so visuals can
# be checked without a GPU. Runs inside the dev container; images go to screenshots/.
# SCREENSHOT_SETTINGS_ONLY=1 captures settings without generating a course or sample rides.
# SCREENSHOT_GRAPHICS_ONLY=1 captures Graphics alone without preparing a kept world.
# SCREENSHOT_RIDE_OPTIONS_ONLY=1 stops after course detail and the ride's settings.
set -eu

root="$(cd "$(dirname "$0")/.." && pwd)"
out="${SCREENSHOT_DIR:-$root/screenshots}"
mkdir -p "$out"
"$root/scripts/build-gdext.sh" debug >/dev/null
# Weeks of made-up rides, so the history and the Analysis tab (R61) have something to show.
if [ "${SCREENSHOT_ERRORS_ONLY:-}" != 1 ] && [ "${SCREENSHOT_SETTINGS_ONLY:-}" != 1 ] \
    && [ "${SCREENSHOT_GROUP_ONLY:-}" != 1 ] \
    && [ "${SCREENSHOT_GROUP_RIDERS_ONLY:-}" != 1 ] \
    && [ "${SCREENSHOT_GRAPHICS_ONLY:-}" != 1 ] \
    && [ "${SCREENSHOT_SUMMARY_ONLY:-}" != 1 ]; then
    cargo run -q --manifest-path "$root/core/Cargo.toml" -p orqwa-cli -- sample-rides --weeks 16
fi
godot --headless --path "$root/app" --import >/dev/null 2>&1 || true
errors="$(godot --headless --path "$root/app" --check-only -s res://tests/screenshots.gd 2>&1 \
    | grep -A3 "SCRIPT ERROR\|SHADER ERROR" || true)"
if [ -n "$errors" ]; then
    echo "$errors" >&2
    exit 1
fi
# Another window size, e.g. SCREENSHOT_SIZE=1024x640, to check that the layouts reflow (R94).
size="${SCREENSHOT_SIZE:-1600x900}"
# Software rendering is slow; a broken script would otherwise leave Godot running forever.
output="$(SCREENSHOT_DIR="$out" ORQWA_RENDER_SOFTWARE=1 timeout 1800 xvfb-run -a -s "-screen 0 ${size}x24" \
    godot --path "$root/app" --rendering-driver vulkan --audio-driver Dummy --resolution "$size" \
    -s res://tests/screenshots.gd 2>&1)" || { echo "$output"; exit 1; }
echo "$output" | grep -E "saved|world generated|overlay window" || true
if echo "$output" | grep -q "SCRIPT ERROR\|SHADER ERROR\|^ERROR:"; then
    echo "$output" | grep -A2 "SCRIPT ERROR\|SHADER ERROR\|^ERROR:" >&2
    exit 1
fi
