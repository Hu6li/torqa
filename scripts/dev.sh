#!/usr/bin/env sh
# Runs a command inside the Torqa dev container (same image as the VS Code devcontainer),
# for use outside VS Code. Example: scripts/dev.sh scripts/check.sh
set -eu

root="$(cd "$(dirname "$0")/.." && pwd)"

docker build -q -t torqa-dev "$root/.devcontainer" >/dev/null

tty_flags=""
if [ -t 0 ] && [ -t 1 ]; then
    tty_flags="-it"
fi

# shellcheck disable=SC2086 # tty_flags is intentionally empty or a single flag
exec docker run --rm $tty_flags \
    -v "$root:/workspaces/torqa" \
    -v torqa-cargo-registry:/usr/local/cargo/registry \
    -v torqa-target:/workspaces/torqa/core/target \
    -w /workspaces/torqa \
    torqa-dev "$@"
