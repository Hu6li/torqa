#!/usr/bin/env sh
# Builds the Rust GDExtension and copies it to where app/torqa.gdextension expects it.
# Usage: scripts/build-gdext.sh [debug|release]
set -eu

profile="${1:-debug}"
root="$(cd "$(dirname "$0")/.." && pwd)"

case "$profile" in
    debug) cargo_flags="" ;;
    release) cargo_flags="--release" ;;
    *) echo "usage: $0 [debug|release]" >&2; exit 1 ;;
esac

case "$(uname -s)" in
    Linux) lib="libtorqa_gd.so" ;;
    Darwin) lib="libtorqa_gd.dylib" ;;
    MINGW* | MSYS* | CYGWIN*) lib="torqa_gd.dll" ;;
    *) echo "unsupported OS: $(uname -s)" >&2; exit 1 ;;
esac

# shellcheck disable=SC2086 # cargo_flags is intentionally empty or a single flag
cargo build --manifest-path "$root/core/Cargo.toml" -p torqa-gd --locked $cargo_flags

mkdir -p "$root/app/bin"
cp "$root/core/target/$profile/$lib" "$root/app/bin/$lib"
echo "Copied $lib ($profile) to app/bin/"
