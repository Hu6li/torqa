#!/usr/bin/env sh
# Builds the Rust GDExtension and copies it to where app/orqwa.gdextension expects it.
# `windows` cross-builds the Windows one in the dev container instead, for the portable Godot
# editor on a Windows PC to render on its GPU (ADR 0021); releases still come from CI.
# Usage: scripts/build-gdext.sh [debug|test|release] [windows]
set -eu

profile="${1:-debug}"
target="${2:-native}"
root="$(cd "$(dirname "$0")/.." && pwd)"

case "$profile" in
    debug) cargo_flags="" ;;
    # Smoke tests share cargo test's optimized dependencies and retain debug assertions.
    test) cargo_flags="--profile test" ;;
    release) cargo_flags="--release" ;;
    *) echo "usage: $0 [debug|test|release] [windows]" >&2; exit 1 ;;
esac
artifact_profile="$profile"
if [ "$profile" = test ]; then artifact_profile=debug; fi

if [ "$target" = windows ]; then
    triple=x86_64-pc-windows-gnu
    clang_include="$(ls -d /usr/lib/llvm-*/lib/clang/*/include | head -n 1)"
    # FFmpeg as the image builds it for Windows; bindgen reads its headers as MinGW's compiler
    # would, with clang's own intrinsics. Linked statically, so the DLL needs no MinGW runtime
    # beside it.
    # shellcheck disable=SC2086 # cargo_flags is intentionally empty or a single flag
    FFMPEG_DIR=/opt/ffmpeg-windows \
        CARGO_TARGET_X86_64_PC_WINDOWS_GNU_LINKER=x86_64-w64-mingw32-gcc \
        CC_x86_64_pc_windows_gnu=x86_64-w64-mingw32-gcc \
        AR_x86_64_pc_windows_gnu=x86_64-w64-mingw32-ar \
        BINDGEN_EXTRA_CLANG_ARGS="--target=x86_64-w64-mingw32 --sysroot=/usr/x86_64-w64-mingw32 -isystem $clang_include" \
        CARGO_TARGET_X86_64_PC_WINDOWS_GNU_RUSTFLAGS="-C target-feature=+crt-static -C link-arg=-static -C link-arg=-static-libgcc" \
        cargo build --manifest-path "$root/core/Cargo.toml" -p orqwa-gd --locked \
        --target "$triple" $cargo_flags
    mkdir -p "$root/app/bin"
    # Without its debug information: hundreds of megabytes the editor does not need.
    x86_64-w64-mingw32-strip -o "$root/app/bin/orqwa_gd.dll" \
        "$root/core/target/$triple/$artifact_profile/orqwa_gd.dll"
    echo "Copied orqwa_gd.dll ($profile, Windows) to app/bin/"
    exit 0
fi

case "$(uname -s)" in
    Linux) lib="liborqwa_gd.so" ;;
    Darwin) lib="liborqwa_gd.dylib" ;;
    MINGW* | MSYS* | CYGWIN*) lib="orqwa_gd.dll" ;;
    *) echo "unsupported OS: $(uname -s)" >&2; exit 1 ;;
esac

# shellcheck disable=SC2086 # cargo_flags is intentionally empty or a single flag
cargo build --manifest-path "$root/core/Cargo.toml" -p orqwa-gd --locked $cargo_flags

mkdir -p "$root/app/bin"
cp "$root/core/target/$artifact_profile/$lib" "$root/app/bin/$lib"
echo "Copied $lib ($profile) to app/bin/"
