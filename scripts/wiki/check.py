"""Check wiki links against the local wiki and application checkout, without network access."""

import re
import subprocess
import sys
from pathlib import Path
from urllib.parse import unquote, urlsplit

OWNERS = ("ORQWA/ORQWA",)
WIKI_URLS = tuple(f"https://github.com/{owner}/wiki" for owner in OWNERS)
REPO_URLS = tuple(f"https://github.com/{owner}/blob/main/" for owner in OWNERS)
RAW_URLS = tuple(f"https://raw.githubusercontent.com/{owner}/main/" for owner in OWNERS)
WIKI_URL = WIKI_URLS[0]
MOVED = "start courses video riding riders hud workouts overlay uploads ghosts history analysis audio cli".split()


def anchors(text):
    """Approximate GitHub heading IDs, including duplicate-heading suffixes."""
    result = set()
    counts = {}
    for heading in re.findall(r"^#{1,6}\s+(.+?)\s*#*\s*$", text, re.M):
        slug = re.sub(r"[^\w\- ]", "", heading.lower()).replace(" ", "-")
        number = counts.get(slug, 0)
        counts[slug] = number + 1
        result.add(slug + (f"-{number}" if number else ""))
    return result


def check(root):
    root = Path(root)
    wiki = root / "wiki"
    pages = {p.stem: p for p in wiki.glob("*.md")}
    errors = []
    if not {"Home", "_Sidebar"}.issubset(pages):
        errors.append("wiki checkout must contain Home.md and _Sidebar.md; run make wiki-init")
    navigation = "\n".join(pages[name].read_text() for name in ("Home", "_Sidebar") if name in pages)
    for name, path in pages.items():
        if not name.startswith("_") and name != "Home" and not any(f"{url}/{name}" in navigation for url in WIKI_URLS):
            errors.append(f"{path}: page missing from navigation")
        text = path.read_text()
        links = re.findall(r"\]\(([^\s)]+)\)", text)
        links += re.findall(r'(?:src|href)=["\']([^"\']+)', text)
        for link in links:
            parsed = urlsplit(link)
            target = None
            if link in WIKI_URLS or link.startswith(tuple(f"{url}/" for url in WIKI_URLS)):
                page = unquote(parsed.path.split("/")[-1]) or "Home"
                if link in WIKI_URLS:
                    page = "Home"
                target = pages.get(page)
                if target is None:
                    errors.append(f"{path}: missing wiki page {link}")
                    continue
            elif link.startswith(REPO_URLS):
                target = root / unquote(parsed.path.split("/blob/main/", 1)[1])
            elif link.startswith(RAW_URLS):
                target = root / unquote(parsed.path.split("/main/", 1)[1])
            elif not parsed.scheme:
                target = path if not parsed.path else wiki / unquote(parsed.path)
                if parsed.path:
                    errors.append(f"{path}: use an absolute wiki/repository URL: {link}")
            if target is not None:
                if not target.exists():
                    errors.append(f"{path}: missing target {link}")
                elif parsed.fragment and target.suffix == ".md" and unquote(parsed.fragment) not in anchors(target.read_text()):
                    errors.append(f"{path}: missing heading {link}")
    # Search tracked text only, excluding the checker and its fixtures, for obsolete usage paths.
    # CI mounts the checkout under another user's UID; git would refuse it as dubious ownership.
    tracked = subprocess.check_output(
        ["git", "-c", f"safe.directory={root}", "ls-files", "-z"], cwd=root
    ).decode().split("\0")
    pattern = re.compile(r"docs/(?:" + "|".join(MOVED) + r")\.md")
    for name in tracked:
        path = root / name
        if not path.is_file() or name.startswith("scripts/wiki/"):
            continue
        try:
            text = path.read_text()
        except UnicodeError:
            continue
        if pattern.search(text):
            errors.append(f"{name}: reference to a migrated docs page")
    return errors


if __name__ == "__main__":
    failures = check(Path(__file__).resolve().parents[2])
    if failures:
        print("\n".join(failures), file=sys.stderr)
        sys.exit(1)
    print("Wiki links, images, headings and navigation checked.")
