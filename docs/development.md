# Development

How to build, check and change Torqa. Contributor conventions are in [CLAUDE.md](../CLAUDE.md),
architecture decisions in [adr/](adr/).

## Setup

All tooling runs in a container — nothing is installed on your machine.

1. Install [Docker](https://www.docker.com/) and [VS Code](https://code.visualstudio.com/) with the
   [Dev Containers](https://marketplace.visualstudio.com/items?itemName=ms-vscode-remote.remote-containers) extension.
2. Open this folder in VS Code and choose **Reopen in Container**.

Without VS Code, `scripts/dev.sh <command>` runs any command in the same container.

## Tasks

| Task | Command (inside the container) |
|---|---|
| All checks (fmt, clippy, tests, cargo-deny, translations, gdlint, gdformat, Godot smoke tests) | `scripts/check.sh` |
| Build the GDExtension into `app/bin/` | `scripts/build-gdext.sh [debug\|release]` |
| Render screenshots of the start page, a ride, the summary and the analysis (software Vulkan, with 16 weeks of made-up rides) into `screenshots/` | `scripts/screenshots.sh` |
| Render the standard views of the 3D world (before and after a visual change) into `screenshots/views/` | `scripts/render-views.sh` |
| Render the 3D models (buildings, plants, clouds) and the riders for review; with `GALLERY=docs/images/models`, also the galleries in `art/README.md` | `scripts/render-models.sh`, `scripts/render-riders.sh` |
| Regenerate the boot splash PNG after a logo change | `scripts/render-splash.sh` |
| Refresh the translation template after text changes | `python3 scripts/i18n/extract.py` |
| Run the CLI with the fake trainer | `cargo run --manifest-path core/Cargo.toml -p torqa-cli -- ride --fake` |

The 3D models are Blender scripts in `art/` (see [art/README.md](../art/README.md) for them and a
gallery of every model); Blender runs in its own container on the host:
`scripts/art.sh blender --background --factory-startup --python art/<group>/build.py`.

## Builds

Release builds of the app and the CLI are artifacts of the
[CI runs](https://github.com/bossm8/Torqa/actions/workflows/ci.yml?query=branch%3Amain) (download
them while signed in to GitHub). There are no tagged releases yet.

The `macos` CI job builds `Torqa-macos-arm64.zip` and `torqa-cli-macos-arm64.tar.gz`. To test
real trainers on macOS, use the CLI built by CI: see [cli.md](cli.md).

Windows builds come from the `windows` CI job: `Torqa-windows-x86_64.zip` (`Torqa.exe` with
`torqa_gd.dll` beside it; keep both in one folder) and `torqa-cli-windows-x86_64.zip`, both under
the run's artifacts. Like the macOS app it is built natively on the runner (MSVC, with MSYS2's
`sh` and `make` for FFmpeg).
FFmpeg's libraries come from `scripts/build-ffmpeg.sh` (a pinned, verified release; ADR 0010):
built into the dev image, and once per platform in CI.

Linux builds come from the `linux` CI job, for x86_64 and arm64: `Torqa-linux-<arch>.tar.gz`
(a `Torqa` folder with `Torqa.<arch>` and `libtorqa_gd.so`; keep both together) and
`torqa-cli-linux-<arch>.tar.gz`. They are built on Ubuntu 22.04 so they run on any distribution
with glibc 2.35 or newer (Ubuntu 22.04+, Debian 12+, Fedora 36+), not only on ones as new as
the dev container ([ADR 0012](adr/0012-linux-builds.md)). CI starts the exported app
headless to check that it loads.

Docker on macOS cannot access Bluetooth or the GPU, so macOS builds are produced by GitHub Actions.
To test 3D rendering or a real trainer, run the built app (or the portable Godot editor) natively.

## More

- Contributor conventions: [CLAUDE.md](../CLAUDE.md)
- Architecture decisions: [adr/](adr/)
- Roadmap: [PLAN.md](PLAN.md); requirements: [REQUIREMENTS.md](REQUIREMENTS.md)
- Third-party assets and their licences: [CREDITS.md](CREDITS.md)
- Translating the app: [translating.md](translating.md)
