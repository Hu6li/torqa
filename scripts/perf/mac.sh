#!/usr/bin/env sh
# Measures frame rates on this Mac's GPU (docs/development.md#measuring-frame-rate): a copy of
# the app in /tmp gets the measuring pack (scripts/perf/pack.sh) and an ad-hoc signature, and
# runs with a throwaway HOME, so the rider's own profile, rides, settings and kept worlds stay as
# they are; it does not look for updates either. Runs on the Mac itself, not in the container:
#   COURSE=<file.oqc|route.gpx> PLACES="<m> ..." scripts/perf/mac.sh <pack> [label]
# APP is the app to copy, /Applications/ORQWA.app unless set; its Rust extension is the one
# measured, so for a Rust change use the macOS app CI built from the branch. QUALITY, CAMERA,
# TAKE, FAR and MINIMAP are app/tools/measure_frames.gd's settings. frames.csv, a screenshot of
# each place and <label>.log go to OUT_DIR, screenshots/perf unless set.
set -eu

if [ "$(uname -s)" != Darwin ]; then
    echo "scripts/perf/mac.sh runs on a Mac" >&2
    exit 1
fi
root="$(cd "$(dirname "$0")/../.." && pwd)"
absolute() { echo "$(cd "$(dirname "$1")" && pwd)/$(basename "$1")"; }
pack="$(absolute "${1:?the pack from scripts/perf/pack.sh}")"
label="${2:-frames}"
course="$(absolute "${COURSE:?a course file or GPX route}")"
app="${APP:-/Applications/ORQWA.app}"
out="${OUT_DIR:-$root/screenshots/perf}"
mkdir -p "$out"
# Godot reads a relative path as one inside the pack.
out="$(cd "$out" && pwd)"
scratch=/tmp/orqwa-measure
copy="$scratch/ORQWA.app"
home="$scratch/home"

rm -rf "$scratch"
mkdir -p "$scratch" "$home/Library/Caches/orqwa"
cp -R "$app" "$copy"
cp "$pack" "$copy/Contents/Resources/ORQWA.pck"
codesign --force --deep --sign - "$copy" 2>/dev/null
printf 'check_updates = false\n' >"$home/Library/Caches/orqwa/computer.toml"

# Under a pseudo-terminal Godot prints as it goes rather than when it quits: follow it with
# `tail -f` on the log.
status=0
script -q "$out/$label.log" env HOME="$home" LABEL="$label" OUT_DIR="$out" TIMED=1 \
    COURSE="$course" PLACES="${PLACES:?distances along the route in metres}" \
    QUALITY="${QUALITY:-ultra}" CAMERA="${CAMERA:-0}" TAKE="${TAKE:-}" FAR="${FAR:-600}" \
    MINIMAP="${MINIMAP:-close}" \
    "$copy/Contents/MacOS/ORQWA" --resolution 1600x900 --windowed </dev/null >/dev/null \
    || status=$?
rm -rf "$scratch"
tr -d '\r' <"$out/$label.log" | grep "^PERF\|SCRIPT ERROR" || true
exit "$status"
