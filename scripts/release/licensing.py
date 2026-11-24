"""A release opens paid and demo worlds only with the maintainers' roots built in and activates
them only with the real activation server (ADR 0043): a tagged build fails while either is still
missing, so no release ships that opens no world or cannot activate one."""

import pathlib
import re
import sys

ROOT = pathlib.Path(__file__).resolve().parents[2]
TRUST = ROOT / "core/orqwa-licence/src/trust.rs"
LICENSING = ROOT / "core/orqwa-app/src/licensing.rs"


def problems(trust: str, licensing: str) -> list[str]:
    """What keeps these sources from being released, by the files' own text."""
    found = []
    roots = re.search(r"const RELEASE_ROOTS: &\[&str\] = &\[(.*?)\];", trust, re.S)
    if roots is None or not roots.group(1).strip():
        found.append("RELEASE_ROOTS in orqwa-licence is empty: add the maintainers' roots")
    server = re.search(r'pub const SERVER: &str = "([^"]*)";', licensing)
    if server is None or not server.group(1).startswith("https://") or ".invalid" in server.group(1):
        found.append("SERVER in orqwa-app's licensing is not the real activation server")
    return found


def main() -> int:
    found = problems(TRUST.read_text(encoding="utf-8"), LICENSING.read_text(encoding="utf-8"))
    for problem in found:
        print(f"error: {problem}", file=sys.stderr)
    return 1 if found else 0


if __name__ == "__main__":
    sys.exit(main())
