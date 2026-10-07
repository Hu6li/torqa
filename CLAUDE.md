# CLAUDE.md

Torqa — offline-first, open-source (GPL-3.0) indoor cycling app. Read before working:
[docs/REQUIREMENTS.md](docs/REQUIREMENTS.md), [docs/PLAN.md](docs/PLAN.md), [docs/adr/](docs/adr/).

## Environment — containers only

- **Never install dependencies natively** on the host (no rustup, cargo, brew, pip, npm, …).
- All development, building, linting and testing runs in Docker via the VS Code devcontainer in
  `.devcontainer/`. Run commands through `docker` / the devcontainer, never on the host.
- Only exception (Docker on macOS has no Bluetooth or Metal): the macOS app and GDExtension are built
  by GitHub Actions macOS runners. Locally, only the portable Godot editor `.app` and downloaded
  Torqa builds run natively, for 3D and real-trainer testing.
- Same for Windows (FFmpeg cannot be cross-compiled from the container): the Windows `.exe` and
  GDExtension are built by the `windows` job on GitHub Actions Windows runners.

## Skills

Project skills live in `.claude/skills/` (see its README): the art direction (`torqa-look`),
the Blender pipeline (`torqa-art-pipeline`), render review (`torqa-render-review`) and imported
Blender references. Use them for any visual or model work.

## Architecture boundary

- All logic lives in Rust (`core/`). Godot (`app/`) is presentation only — no physics, device or
  data logic in GDScript.
- New capabilities go behind the plugin traits (`RouteImporter`, `TrainerDriver`, `SensorDriver`,
  `ShiftInput`, `ActivityUploader`, `WorkoutParser`) so implementations stay swappable.

## Rust conventions

- Errors: `thiserror` in library crates, `anyhow` only in binaries (`torqa-cli`, gdext glue).
- Async: `tokio`. Logging: `tracing`.
- Physical quantities as custom newtypes (`Watts`, `Meters`, `MetersPerSecond`, `Kilograms`, …),
  SI units internally; convert to imperial only at the presentation edge.
- `rustfmt`; `clippy::pedantic` with justified `#[allow]`s; warnings fail CI; `missing_docs` on
  public items.

## GDScript conventions

- Statically typed everywhere, warnings as errors; `gdlint` + `gdformat`.
- User-facing text goes through `tr()` before formatting; regenerate the template with
  `scripts/i18n/extract.py` and keep every translation complete (see docs/translating.md).

## Testing

- Test the behaviour we *intend* (requirements, R-IDs) — not incidental current output. Do not
  write tests that merely freeze whatever the code happens to do.
- Core logic changes come with tests; hardware paths are tested via the fake trainer.

## Dependencies

- `cargo-deny` enforces GPL-compatible licenses and no unmaintained crates.
- **Ask before adding any dependency** (crate, Godot plugin, container package).
- **Always use the most recent stable version** of everything: crates, container base images, tools,
  Godot, GitHub Actions. Look up the current release online before adding or touching a version —
  never rely on remembered version numbers.

## Formats

- TOML for configuration, JSON for data/metadata, FIT for activities.

## Documentation

- Comments explain *why*, never narrate *what*.
- Rustdoc on all public items.
- Significant decisions get an ADR in `docs/adr/`.
- User-facing docs / README per feature.

## Git

- GitHub, trunk-based, short-lived branches, PRs, squash-merge.
- Conventional Commits (`feat:`, `fix:`, `docs:`, `refactor:`, `test:`, `chore:`, `ci:`).

## Working agreement for Claude

- Small PR-sized steps, one roadmap phase at a time; stop for review after each meaningful step.
- Before declaring done, run in the container: `cargo fmt --check`,
  `cargo clippy --all-targets -- -D warnings`, `cargo test`, `cargo deny check`, and `gdlint`.
- Keep the checkboxes in `docs/PLAN.md` up to date.
