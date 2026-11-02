#!/usr/bin/env sh
# Fuzzes the decoders that read untrusted group-ride input (ADR 0035): codes, lobby and
# handover controls, voice and state datagrams, selections, workouts, previews, STUN and
# shared courses.
# Each target runs for the given number of seconds (default 30). Inside the dev container;
# CI runs it on every change. A finding stops with its input under core/fuzz/artifacts/.
set -eu

seconds="${1:-30}"
root="$(cd "$(dirname "$0")/.." && pwd)"
corpus="$root/core/fuzz/corpus"

# From core/, so the stable toolchain lints; inside core/fuzz rust-toolchain.toml picks nightly.
cd "$root/core"
cargo fmt --manifest-path fuzz/Cargo.toml --check
cargo clippy --manifest-path fuzz/Cargo.toml --all-targets --locked -- -D warnings
# Start from today's valid encodings, so a format change cannot leave only stale seeds.
ORQWA_FUZZ_CORPUS="$corpus" cargo test --locked -q -p orqwa-net --features fuzzing --lib \
    fuzz::tests::write_fuzz_seeds -- --ignored --exact
ORQWA_FUZZ_CORPUS="$corpus" cargo test --locked -q -p orqwa-storage --lib \
    course::shared::tests::write_fuzz_seed -- --ignored --exact

cd "$root/core/fuzz"
for target in $(cargo fuzz list); do
    echo "Fuzzing $target for ${seconds}s"
    mkdir -p "$corpus/$target"
    cargo fuzz run "$target" "$corpus/$target" -- \
        -max_total_time="$seconds" -timeout=10 -rss_limit_mb=2048 -print_final_stats=1
done
