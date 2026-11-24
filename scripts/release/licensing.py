"""A release opens the maintainers' worlds only with their roots built in (ADR 0043): a tagged
build fails while none is, so no release ships that opens no world."""

import pathlib
import re
import sys

ROOT = pathlib.Path(__file__).resolve().parents[2]
TRUST = ROOT / "core/orqwa-licence/src/trust.rs"


def problems(trust: str) -> list[str]:
    """What keeps these sources from being released, by the files' own text."""
    found = []
    roots = re.search(r"const RELEASE_ROOTS: &\[&str\] = &\[(.*?)\];", trust, re.S)
    if roots is None or not roots.group(1).strip():
        found.append("RELEASE_ROOTS in orqwa-licence is empty: add the maintainers' roots")
    return found


def main() -> int:
    found = problems(TRUST.read_text(encoding="utf-8"))
    for problem in found:
        print(f"error: {problem}", file=sys.stderr)
    return 1 if found else 0


if __name__ == "__main__":
    sys.exit(main())
