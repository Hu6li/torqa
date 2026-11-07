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
checks=true
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
    checks=false
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
            scripts/check.sh | scripts/checks/* | scripts/build-gdext.sh | .github/workflows/ci.yml)
                checks=true; godot=true ;;
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
if $checks; then
    python3 -m unittest discover -s "$root/scripts/checks" -p 'test_*.py'
fi
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

# Reuse the optimized test dependencies for the extension as well as the Rust tests.
"$root/scripts/build-gdext.sh" test
python3 "$root/scripts/checks/godot.py"
