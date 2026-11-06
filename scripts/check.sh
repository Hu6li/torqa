#!/usr/bin/env sh
# All checks that must pass before a change is done. Runs inside the dev container.
# `--quick` runs only the checks the branch's changes can affect (since it left main, committed
# or not), for quick turns while working; the full run stays for before a PR, and CI runs it.
set -eu

root="$(cd "$(dirname "$0")/.." && pwd)"

quick=false
if [ "${1:-}" = "--quick" ]; then
    quick=true
fi

# What the checks below depend on: in a full run everything, in a quick run what changed.
rust=true
crates=""
deps=true
release=true
i18n=true
wiki=true
perf=true
gd_files=""
godot=true
if $quick; then
    git_() { git -c safe.directory="$root" -C "$root" "$@"; }
    base="$(git_ merge-base HEAD origin/main)"
    changed="$( (git_ diff --name-only "$base"; git_ ls-files --others --exclude-standard) | sort -u)"
    rust=false
    deps=false
    release=false
    i18n=false
    wiki=false
    perf=false
    godot=false
    for path in $changed; do
        case "$path" in
            core/Cargo.lock | core/Cargo.toml | core/deny.toml | core/about.*)
                rust=true; deps=true; release=true ;;
            core/fuzz/*) rust=true; deps=true ;;
            core/orqwa-*/*)
                rust=true; crates="$crates $(echo "$path" | cut -d/ -f2)" ;;
            core/*) rust=true ;;
        esac
        # Files the crates embed (include_str!/include_bytes!): a change is one to the crate.
        case "$path" in
            app/assets/palette.json | app/assets/models/*/models.json)
                rust=true; crates="$crates orqwa-world" ;;
            app/assets/brand/*) rust=true; crates="$crates orqwa-remote" ;;
            LICENSE) rust=true ;;
        esac
        case "$path" in
            *.gd) gd_files="$gd_files $path" ;;
        esac
        case "$path" in
            app/*.gd | app/*.tscn | app/translations/* | scripts/i18n/*) i18n=true ;;
        esac
        case "$path" in
            app/*) godot=true ;;
        esac
        case "$path" in
            scripts/perf/*) perf=true ;;
        esac
        case "$path" in
            wiki | docs/* | README.md | scripts/wiki/*) wiki=true ;;
        esac
        case "$path" in
            app/export_presets.cfg | scripts/release/*) release=true ;;
        esac
    done
    # The Godot checks load the extension: a Rust change can break them as well.
    if $rust; then
        godot=true
    fi
    crates="$(echo "$crates" | tr ' ' '\n' | sort -u | tr '\n' ' ')"
    echo "Quick checks for: ${changed:-nothing}" | tr '\n' ' '
    echo
fi

cd "$root/core"
if $wiki; then
    python3 "$root/scripts/wiki/check.py"
    python3 -m unittest discover -s "$root/scripts/wiki" -p 'test_*.py'
fi
if $perf; then
    python3 -m unittest discover -s "$root/scripts/perf" -p 'test_*.py'
fi
if $release; then
    python3 -m unittest discover -s "$root/scripts/release" -p 'test_*.py'
    # The Cargo workspace and the macOS bundle name the same version; release tags check it too.
    python3 "$root/scripts/release/version.py"
fi
if $rust; then
    cargo fmt --all --check
    # The fuzz targets are a workspace of their own (scripts/fuzz.sh builds and runs them).
    cargo fmt --manifest-path fuzz/Cargo.toml --check
    cargo clippy --workspace --all-targets --locked -- -D warnings
    if $quick && [ -n "$(echo "$crates" | tr -d ' ')" ] && ! $deps; then
        # Only the changed crates' tests; the full run also tests the crates depending on them.
        # shellcheck disable=SC2046 # one -p per crate
        cargo test --locked $(for crate in $crates; do printf -- '-p %s ' "$crate"; done)
    else
        cargo test --workspace --locked
    fi
fi
if $deps; then
    cargo deny --all-features check
    cargo deny --manifest-path fuzz/Cargo.toml --config deny.toml check
    # The crates' licence notices the About tab shows are those of today's dependencies (R83).
    "$root/scripts/notices.sh" --check
fi
if $i18n; then
    # Translatable texts: template up to date, every translation complete (R24).
    python3 "$root/scripts/i18n/extract.py" --check
    python3 -m unittest discover -s "$root/scripts/i18n" -p 'test_*.py'
fi

cd "$root/app"
if ! $quick; then
    gdlint .
    gdformat --check .
elif [ -n "$gd_files" ]; then
    # Deleted scripts are listed as changed too.
    existing=""
    for path in $gd_files; do
        if [ -f "$root/$path" ]; then
            existing="$existing $root/$path"
        fi
    done
    if [ -n "$existing" ]; then
        # shellcheck disable=SC2086 # one argument per file
        gdlint $existing
        # shellcheck disable=SC2086
        gdformat --check $existing
    fi
fi

if ! $godot; then
    echo "Quick checks passed (no Godot checks needed)"
    exit 0
fi

# Godot checks: the extension loads, all scripts compile (warnings are errors) and the
# ORQWA node API works end to end.
"$root/scripts/build-gdext.sh" debug
godot --headless --path "$root/app" --import >/dev/null 2>&1 || true

run_godot() {
    output="$(godot --headless --path "$root/app" "$@" 2>&1)" || { echo "$output"; return 1; }
    echo "$output"
    if echo "$output" | grep -q "SCRIPT ERROR\|SHADER ERROR\|^ERROR:"; then
        # The callers capture standard output, so the errors go to standard error as well:
        # otherwise a failing check only says that Godot reported errors, not which.
        echo "$output" | grep -A3 "SCRIPT ERROR\|SHADER ERROR\|^ERROR:" >&2
        echo "Godot reported errors" >&2
        return 1
    fi
}
# Capture first: in a pipeline, sh would only see grep's exit status.
main_output="$(run_godot --quit-after 10)"
echo "$main_output" | grep -q "^ORQWA "
echo "Main scene smoke test passed"
ride_output="$(run_godot -s res://tests/ride_smoke.gd)"
echo "$ride_output" | grep "RIDE SMOKE TEST PASSED"
ui_output="$(run_godot -s res://tests/ui_smoke.gd)"
echo "$ui_output" | grep "UI SMOKE TEST PASSED"
group_ui_output="$(run_godot -s res://tests/group_ui_smoke.gd)"
echo "$group_ui_output" | grep "GROUP UI SMOKE TEST PASSED"
seasons_output="$(run_godot -s res://tests/seasons_smoke.gd)"
echo "$seasons_output" | grep "SEASONS SMOKE TEST PASSED"
touch_output="$(run_godot -s res://tests/touch_smoke.gd)"
echo "$touch_output" | grep "TOUCH SMOKE TEST PASSED"
summary_output="$(run_godot -s res://tests/summary_smoke.gd)"
echo "$summary_output" | grep "SUMMARY SMOKE TEST PASSED"
workouts_output="$(run_godot -s res://tests/workouts_smoke.gd)"
echo "$workouts_output" | grep "WORKOUTS SMOKE TEST PASSED"
devices_output="$(run_godot -s res://tests/devices_smoke.gd)"
echo "$devices_output" | grep "DEVICES SMOKE TEST PASSED"
window_output="$(run_godot -s res://tests/window_smoke.gd)"
echo "$window_output" | grep "WINDOW SMOKE TEST PASSED"
dialog_output="$(run_godot -s res://tests/dialog_smoke.gd)"
echo "$dialog_output" | grep "DIALOG SMOKE TEST PASSED"
settings_output="$(run_godot -s res://tests/settings_smoke.gd)"
echo "$settings_output" | grep "SETTINGS SMOKE TEST PASSED"
benchmark_output="$(run_godot -s res://tests/benchmark_smoke.gd)"
echo "$benchmark_output" | grep "BENCHMARK SMOKE TEST PASSED"
tree_lod_output="$(run_godot -s res://tests/tree_lod_smoke.gd)"
echo "$tree_lod_output" | grep "TREE LOD SMOKE TEST PASSED"
hud_output="$(run_godot -s res://tests/hud_smoke.gd)"
echo "$hud_output" | grep "HUD SMOKE TEST PASSED"
course_output="$(run_godot -s res://tests/course_smoke.gd)"
echo "$course_output" | grep "COURSE SMOKE TEST PASSED"
loading_output="$(run_godot -s res://tests/course_loading_smoke.gd)"
echo "$loading_output" | grep "COURSE LOADING SMOKE TEST PASSED"
rider_output="$(run_godot -s res://tests/rider_smoke.gd)"
echo "$rider_output" | grep "RIDER SMOKE TEST PASSED"
profile_output="$(run_godot -s res://tests/profile_smoke.gd)"
echo "$profile_output" | grep "PROFILE SMOKE TEST PASSED"
analysis_output="$(run_godot -s res://tests/analysis_smoke.gd)"
echo "$analysis_output" | grep "ANALYSIS SMOKE TEST PASSED"
upload_output="$(run_godot -s res://tests/upload_smoke.gd)"
echo "$upload_output" | grep "UPLOAD SMOKE TEST PASSED"
error_output="$(run_godot -s res://tests/error_smoke.gd)"
echo "$error_output" | grep "ERROR SMOKE TEST PASSED"
moment_output="$(run_godot -s res://tests/moment_smoke.gd)"
echo "$moment_output" | grep "MOMENT SMOKE TEST PASSED"

# Headless smoke tests use Godot's dummy renderer and cannot catch back-face culling.
surface_output="$(timeout 120 xvfb-run -a godot --path "$root/app" --rendering-driver vulkan \
    --audio-driver Dummy --resolution 800x240 -s res://tests/surface_smoke.gd 2>&1)" \
    || { echo "$surface_output"; exit 1; }
echo "$surface_output" | grep "SURFACE SMOKE TEST PASSED"
if echo "$surface_output" | grep -q "SCRIPT ERROR\|SHADER ERROR\|^ERROR:"; then
    echo "$surface_output" >&2
    exit 1
fi

# UI text must bypass world post-processing while its scenery visibility remains correct.
nameplate_output="$(timeout 90 xvfb-run -a godot --path "$root/app" --rendering-driver vulkan \
    --audio-driver Dummy --resolution 800x450 -s res://tests/nameplate_render_smoke.gd 2>&1)" \
    || { echo "$nameplate_output"; exit 1; }
echo "$nameplate_output" | grep "NAMEPLATE RENDER SMOKE TEST PASSED"
if echo "$nameplate_output" | grep -q "SCRIPT ERROR\|SHADER ERROR\|^ERROR:"; then
    echo "$nameplate_output" >&2
    exit 1
fi

# Mouse visibility is a display feature too: the headless backend has no mouse mode.
pointer_output="$(timeout 90 xvfb-run -a godot --path "$root/app" --audio-driver Dummy \
    -s res://tests/idle_pointer_smoke.gd 2>&1)" \
    || { echo "$pointer_output"; exit 1; }
echo "$pointer_output" | grep "IDLE POINTER SMOKE TEST PASSED"
if echo "$pointer_output" | grep -q "SCRIPT ERROR\|SHADER ERROR\|^ERROR:"; then
    echo "$pointer_output" >&2
    exit 1
fi
