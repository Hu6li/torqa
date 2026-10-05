#!/usr/bin/env sh
# All checks that must pass before a change is done. Runs inside the dev container.
set -eu

root="$(cd "$(dirname "$0")/.." && pwd)"

cd "$root/core"
cargo fmt --all --check
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo test --workspace --locked
cargo deny --all-features check
# Translatable texts: template up to date, every translation complete (R24).
python3 "$root/scripts/i18n/extract.py" --check

cd "$root/app"
gdlint .
gdformat --check .

# Godot checks: the extension loads, all scripts compile (warnings are errors) and the
# Torqa node API works end to end.
"$root/scripts/build-gdext.sh" debug
godot --headless --path "$root/app" --import >/dev/null 2>&1 || true

run_godot() {
    output="$(godot --headless --path "$root/app" "$@" 2>&1)" || { echo "$output"; return 1; }
    echo "$output"
    if echo "$output" | grep -q "SCRIPT ERROR\|SHADER ERROR\|^ERROR:"; then
        echo "Godot reported errors" >&2
        return 1
    fi
}
# Capture first: in a pipeline, sh would only see grep's exit status.
main_output="$(run_godot --quit-after 10)"
echo "$main_output" | grep -q "^Torqa "
echo "Main scene smoke test passed"
ride_output="$(run_godot -s res://tests/ride_smoke.gd)"
echo "$ride_output" | grep "RIDE SMOKE TEST PASSED"
ui_output="$(run_godot -s res://tests/ui_smoke.gd)"
echo "$ui_output" | grep "UI SMOKE TEST PASSED"
rider_output="$(run_godot -s res://tests/rider_smoke.gd)"
echo "$rider_output" | grep "RIDER SMOKE TEST PASSED"
