#!/usr/bin/env sh
# Runs a command in the Torqa art container (headless Blender, ADR 0009). Example, to rebuild
# the building models: scripts/art.sh blender --background --factory-startup --python art/buildings/build.py
set -eu

root="$(cd "$(dirname "$0")/.." && pwd)"

docker build -q --platform linux/amd64 -t torqa-art "$root/art" >/dev/null

exec docker run --rm --platform linux/amd64 \
    -v "$root:/workspaces/torqa" \
    -w /workspaces/torqa \
    torqa-art "$@"
