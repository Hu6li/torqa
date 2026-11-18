#!/usr/bin/env sh
# Renders the showcase shots of shots.txt on this computer's GPU (macOS) and exports them as
# the gallery's JPEGs. GODOT=<Godot binary> [COURSES=<dir>] [ONLY="name …"] [OUT_DIR=<dir>]
# .claude/skills/orqwa-showcase/render.sh — needs the ORQWA extension of the checked-out
# commit in app/bin/ (see SKILL.md). Full-size PNGs stay in OUT_DIR for review.
set -eu

here="$(cd "$(dirname "$0")" && pwd)"
root="$(cd "$here/../../.." && pwd)"
courses="${COURSES:-$HOME/Library/Application Support/ORQWA/courses}"
out="${OUT_DIR:-$root/screenshots/showcase}"
gallery="$root/docs/images/showcase"
mkdir -p "$out" "$gallery"
# One run per course: its world is built (or loaded from the kept worlds) once for all shots.
for course in $(grep -v '^#' "$here/shots.txt" | awk 'NF { print $1 }' | uniq); do
    shots=""
    for shot in $(grep -v '^#' "$here/shots.txt" | awk -v c="$course" '$1 == c { print $2 }'); do
        name="${shot%%:*}"
        if [ -z "${ONLY:-}" ] || echo " $ONLY " | grep -q " $name "; then
            shots="$shots $shot"
        fi
    done
    [ -n "$shots" ] || continue
    echo "== $course:$shots"
    COURSE="$courses/$course.oqc" SHOTS="$shots" OUT_DIR="$out" "$root/scripts/render-showcase.sh"
    for shot in $shots; do
        name="${shot%%:*}"
        sips -Z 1600 -s format jpeg -s formatOptions 78 "$out/$name.png" \
            --out "$gallery/$name.jpg" >/dev/null
    done
done
