"""Publish only wiki history referenced by application main; reconcile its gitlink via a PR."""

import json
import os
import subprocess
import tempfile
from pathlib import Path
from urllib.parse import urlencode
from urllib.request import Request, urlopen

from check import check

SYNC_BRANCH = "automation/wiki-sync"
TITLE = "docs: sync published wiki revision"


def git(path, *args):
    # Credentials are read from the environment by Git, never put in URLs or logs.
    credential = '!f() { echo username=x-access-token; echo "password=$GH_TOKEN"; }; f'
    return subprocess.check_output(
        ["git", "-c", "credential.helper=", "-c", f"credential.helper={credential}",
         "-C", str(path), *args], text=True,
    ).strip()


def ancestor(path, before, after):
    result = subprocess.run(["git", "-C", str(path), "merge-base", "--is-ancestor", before, after])
    if result.returncode not in (0, 1):
        raise RuntimeError("Could not inspect wiki ancestry")
    return result.returncode == 0


def identity(path):
    git(path, "config", "user.name", "ORQWA wiki automation")
    git(path, "config", "user.email", "41898282+github-actions[bot]@users.noreply.github.com")


def publish_history(wiki, target, published_branch):
    """Merge locally first; a conflicting or rejected push never overwrites remote edits."""
    before = git(wiki, "rev-parse", f"origin/{published_branch}")
    git(wiki, "checkout", "--detach", before)
    if not ancestor(wiki, target, before):
        if ancestor(wiki, before, target):
            git(wiki, "merge", "--ff-only", target)
        else:
            try:
                git(wiki, "merge", "--no-edit", target)
            except subprocess.CalledProcessError:
                git(wiki, "merge", "--abort")
                raise RuntimeError("Wiki merge conflict: reconcile the wiki preparation branch and retry") from None
    return before, git(wiki, "rev-parse", "HEAD")


def prune_branches(wiki, published):
    """Delete preparation branches whose history is published; their commits stay reachable."""
    refs = git(wiki, "for-each-ref", "--format=%(refname:strip=3) %(objectname)", "refs/remotes/origin/docs")
    for line in refs.splitlines():
        name, revision = line.split()
        if not ancestor(wiki, revision, published):
            continue
        try:
            # The lease keeps a branch that received new commits since the fetch.
            git(wiki, "push", f"--force-with-lease=refs/heads/{name}:{revision}", "origin", f":refs/heads/{name}")
        except subprocess.CalledProcessError:
            print(f"Kept wiki branch {name}: it changed during publication")


class GitHub:
    def __init__(self, repository, token):
        self.repository = repository
        self.token = token

    def request(self, method, endpoint, data=None):
        request = Request(
            f"https://api.github.com/repos/{self.repository}/{endpoint}",
            data=json.dumps(data).encode() if data is not None else None,
            headers={"Authorization": f"Bearer {self.token}", "Accept": "application/vnd.github+json"},
            method=method,
        )
        with urlopen(request, timeout=30) as response:
            return json.load(response)

    def reconcile_pr(self, needed, body):
        query = urlencode({"state": "open", "head": f"{self.repository.split('/')[0]}:{SYNC_BRANCH}", "base": "main"})
        existing = self.request("GET", f"pulls?{query}")
        if not needed:
            for pr in existing:
                self.request("PATCH", f"pulls/{pr['number']}", {"state": "closed"})
            return
        data = {"title": TITLE, "body": body}
        if existing:
            self.request("PATCH", f"pulls/{existing[0]['number']}", data)
        else:
            self.request("POST", "pulls", {**data, "head": SYNC_BRANCH, "base": "main"})


# Only the current wiki may receive a prepared revision.
WIKI_REMOTES = (
    "https://github.com/ORQWA/ORQWA.wiki.git",
)


def publish(application_remote, api):
    # Work in disposable clones: local agent work and the workflow checkout remain untouched.
    with tempfile.TemporaryDirectory(prefix="orqwa-wiki-publish-") as directory:
        app = Path(directory) / "application"
        git(directory, "clone", "--no-checkout", application_remote, str(app))
        identity(app)
        for attempt in range(3):
            git(app, "fetch", "origin", "+refs/heads/main:refs/remotes/origin/main")
            git(app, "checkout", "-B", SYNC_BRANCH, "origin/main")
            main = git(app, "rev-parse", "HEAD")
            entry = git(app, "ls-tree", "HEAD", "wiki").split()
            if len(entry) < 3 or entry[0] != "160000":
                raise RuntimeError("Application main does not contain a wiki submodule")
            target = entry[2]
            remote = git(app, "config", "-f", ".gitmodules", "--get", "submodule.wiki.url")
            if remote not in WIKI_REMOTES and not os.environ.get("WIKI_TEST_REMOTES"):
                raise RuntimeError("Unexpected wiki remote; refusing to publish")
            wiki = app / "wiki"
            if not (wiki / ".git").exists():
                git(app, "clone", remote, str(wiki))
                identity(wiki)
            git(wiki, "fetch", "--prune", "origin", "+refs/heads/*:refs/remotes/origin/*")
            published_branch = git(wiki, "symbolic-ref", "--short", "refs/remotes/origin/HEAD").removeprefix("origin/")
            git(wiki, "cat-file", "-e", f"{target}^{{commit}}")
            before, published = publish_history(wiki, target, published_branch)
            failures = check(app)
            if failures:
                raise RuntimeError("Wiki validation failed:\n" + "\n".join(failures))
            git(app, "fetch", "origin", "+refs/heads/main:refs/remotes/origin/main")
            if git(app, "rev-parse", "origin/main") != main:
                continue
            if published != before:
                git(wiki, "push", "origin", f"HEAD:refs/heads/{published_branch}")
            print(f"Published wiki {published} for application {main}")
            prune_branches(wiki, published)
            # Main may advance during the push; re-read it before constructing the pointer PR.
            git(app, "fetch", "origin", "+refs/heads/main:refs/remotes/origin/main")
            if git(app, "rev-parse", "origin/main") != main:
                continue
            needed = published != target
            if needed:
                # The automation owns this one branch. A lease protects concurrent edits.
                refs = git(app, "ls-remote", "origin", f"refs/heads/{SYNC_BRANCH}")
                expected = refs.split()[0] if refs else ""
                reusable = False
                if expected:
                    git(app, "fetch", "origin", f"refs/heads/{SYNC_BRANCH}")
                    reusable = (git(app, "rev-parse", f"{expected}^") == main
                                and git(app, "rev-parse", f"{expected}:wiki") == published)
                if not reusable:
                    git(app, "update-index", "--cacheinfo", f"160000,{published},wiki")
                    git(app, "commit", "-m", TITLE)
                    git(app, "push", f"--force-with-lease=refs/heads/{SYNC_BRANCH}:{expected}",
                        "origin", f"HEAD:refs/heads/{SYNC_BRANCH}")
            body = (
                "The usage wiki was published after the application change reached `main`. "
                "This PR updates only its submodule revision to include existing published wiki edits.\n\n"
                f"Application revision: `{main}`\n\nPublished wiki revision: `{published}`\n\n"
                "Validation: wiki links, images, headings and navigation passed."
            )
            api.reconcile_pr(needed, body)
            return published
        raise RuntimeError("Application main kept advancing; retry publication against the latest main")


if __name__ == "__main__":
    repository = os.environ.get("GITHUB_REPOSITORY", "ORQWA/ORQWA")
    token = os.environ.get("GH_TOKEN")
    if not token:
        raise SystemExit("Pass the workflow's automatic GitHub token in GH_TOKEN")
    publish(f"https://github.com/{repository}.git", GitHub(repository, token))
