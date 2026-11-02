#!/usr/bin/env sh
# Writes the app's About notices (R83, ADR 0018) and the CLI's redistributable licence file.
# Run it after changing dependencies;
# with --check, it fails if the file is out of date instead. Runs inside the dev container.
# Usage: scripts/notices.sh [--check]
set -eu

root="$(cd "$(dirname "$0")/.." && pwd)"
notices="$root/core/orqwa-app/notices/crates.txt"
generated="$(mktemp)"
trap 'rm -f "$generated"' EXIT

# The extension's dependencies, not the workspace's: the CLI's and the tests' stay out.
cargo about generate --fail --locked \
    -c "$root/core/about.toml" -m "$root/core/orqwa-gd/Cargo.toml" \
    -o "$generated" "$root/core/about.hbs"

printf '\nBundled libopus 1.6.1\n\n' >>"$generated"
cat "$root/core/orqwa-voice/notices/opus.txt" >>"$generated"
python3 - "$generated" <<'PY'
from pathlib import Path
import sys
path = Path(sys.argv[1])
path.write_text("".join(line.rstrip() + "\n" for line in path.read_text().splitlines()), newline="\n")
PY

case "${1:-}" in
    --check)
        if ! cmp -s "$generated" "$notices"; then
            echo "core/orqwa-app/notices/crates.txt is out of date: run scripts/notices.sh" >&2
            exit 1
        fi
        ;;
    "") cat "$generated" >"$notices" ;;
    *) echo "usage: $0 [--check]" >&2; exit 1 ;;
esac

# Both front ends carry the voice and native audio dependencies.
# libopus is bundled C code; its own notice accompanies the Rust wrapper's licence too.
notices="$root/core/orqwa-cli/notices/VOICE-LICENSES.txt"
cargo about generate --fail --locked \
    -c "$root/core/about.toml" -m "$root/core/orqwa-cli/Cargo.toml" \
    -o "$generated" "$root/core/about.hbs"
printf '\nBundled libopus 1.6.1\n\n' >>"$generated"
cat "$root/core/orqwa-voice/notices/opus.txt" >>"$generated"
# Third-party licence files can carry CRLF or trailing spaces. Keep the words intact while
# making generation match Git's LF checkout on every platform.
python3 - "$generated" <<'PY'
from pathlib import Path
import sys

path = Path(sys.argv[1])
path.write_text("".join(line.rstrip() + "\n" for line in path.read_text().splitlines()), newline="\n")
PY
if [ "${1:-}" = --check ]; then
    if ! cmp -s "$generated" "$notices"; then
        echo 'CLI voice licences are out of date: run scripts/notices.sh' >&2
        exit 1
    fi
else
    mkdir -p "$(dirname "$notices")"
    cat "$generated" >"$notices"
fi
