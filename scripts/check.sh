#!/usr/bin/env sh
# All checks that must pass before a change is done. Runs inside the dev container.
set -eu

root="$(cd "$(dirname "$0")/.." && pwd)"

cd "$root/core"
cargo fmt --all --check
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo test --workspace --locked
cargo deny --all-features check

cd "$root/app"
gdlint .
gdformat --check .

# Smoke test: the GDExtension loads and GDScript can call into Rust.
"$root/scripts/build-gdext.sh" debug
godot --headless --path "$root/app" --import >/dev/null 2>&1 || true
godot --headless --path "$root/app" --quit-after 5 2>&1 | tee /dev/stderr | grep -q "^Torqa "
echo "GDExtension smoke test passed"
