#!/usr/bin/env sh
# Renders showcase pictures (website, README, gallery) of a saved course on this computer's GPU
# with the portable Godot editor, at the Ultra preset and 3200x1800, unlike the software
# renders of the dev container (ADR 0021). Needs the ORQWA extension for this platform in
# app/bin/ (a release build's, or scripts/build-gdext.sh release windows).
# GODOT=/path/to/Godot COURSE=<file.oqc> SHOTS="name:distance:camera[:right,up,back][:key=value]…"
# scripts/render-showcase.sh — keys: time=Morning|Midday|Evening, weather=Clear|Cloudy|Hazy|Rain,
# season=Spring|Summer|Autumn|Winter, rider=female|male, group=<friends riding along, up to 7>,
# hud=0, fov=<degrees>, elevation=gradient, map=dark (see app/tools/render_views.gd). A separate HOME keeps the rider's profile and settings out of
# it while reusing their downloaded map, terrain and kept-world caches.
set -eu

root="$(cd "$(dirname "$0")/.." && pwd)"
out="${OUT_DIR:-$root/screenshots/showcase}"
home="${SHOWCASE_HOME:-$root/screenshots/showcase-home}"
case "$(uname -s)" in
    Darwin) caches="$HOME/Library/Caches/ORQWA" ; isolated="$home/Library/Caches/ORQWA" ;;
    *) caches="${XDG_CACHE_HOME:-$HOME/.cache}/orqwa" ; isolated="$home/.cache/orqwa" ;;
esac
mkdir -p "$out" "$isolated"
for cache in osm terrain imagery worlds; do
    if [ -d "$caches/$cache" ]; then
        ln -sfn "$caches/$cache" "$isolated/$cache"
    fi
done
HOME="$home" XDG_CACHE_HOME="$home/.cache" XDG_DATA_HOME="$home/.local/share" \
    "$GODOT" --headless --path "$root/app" --import >/dev/null 2>&1 || true
HOME="$home" XDG_CACHE_HOME="$home/.cache" XDG_DATA_HOME="$home/.local/share" SHOWCASE=1 \
    ROUTE="$(cd "$(dirname "$COURSE")" && pwd)/$(basename "$COURSE")" OUT_DIR="$out" \
    QUALITY="${QUALITY:-ultra}" "$GODOT" --path "$root/app" -s res://tools/render_views.gd 2>&1 \
    | grep -E "^saved|SCRIPT ERROR|SHADER ERROR|Cannot"
