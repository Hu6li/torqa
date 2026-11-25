# CLAUDE.md

ORQWA — offline-first, open-source (GPL-3.0) indoor cycling app. Read before working:
[docs/REQUIREMENTS.md](docs/REQUIREMENTS.md), [docs/PLAN.md](docs/PLAN.md), [docs/adr/](docs/adr/).

## Environment — containers only

- **Never install dependencies natively** on the host (no rustup, cargo, brew, pip, npm, …).
- All development, building, linting and testing runs in Docker via the VS Code devcontainer in
  `.devcontainer/`. Run commands through `docker` / the devcontainer, never on the host.
- Only exception (Docker on macOS has no Bluetooth or Metal): the macOS app and GDExtension are built
  by GitHub Actions macOS runners. Locally, only the portable Godot editor `.app` and downloaded
  ORQWA builds run natively, for 3D and real-trainer testing.
- Same for Windows (a Windows DLL with Windows' Bluetooth behind it): the Windows `.exe` and
  GDExtension are built by the `windows` job on GitHub Actions Windows runners. For native GPU
  renders during development, `scripts/build-gdext.sh release windows` cross-builds the DLL in
  the container into `app/bin/` for the portable Godot editor (ADR 0021); releases stay on CI.
- Linux release builds come from the `linux` job on GitHub Actions `ubuntu-22.04` runners, not
  the container: its newer glibc would keep them from starting on older distributions (ADR 0012).
  Linux development and checks still run in the container.

## Skills

Project skills live in `.claude/skills/` (see its README): the art direction (`orqwa-look`),
the Blender pipeline (`orqwa-art-pipeline`), render review (`orqwa-render-review`) and imported
Blender references. Use them for any visual or model work. The imported `humanizer` skill edits
prose: use it on docs, the wiki and UI strings.

## Architecture boundary

- All logic lives in Rust (`core/`). Godot (`app/`) is presentation only — no physics, device or
  data logic in GDScript.
- New capabilities go behind the plugin traits (`RouteImporter`, `TrainerDriver`, `SensorDriver`,
  `ShiftInput`, `ActivityUploader`, `WorkoutParser`) so implementations stay swappable.
- Riders may keep built worlds across updates (R101, ADR 0025). A change that makes courses come
  out differently raises `orqwa_world::GENERATOR`; one that changes the stored world's layout or
  what the app makes of it (fields, instance buffers, model names) raises
  `orqwa_world::stored::FORMAT`.
- **World files (`.oqx`, ADR 0042) stay working for good.** They are sold, and riders expect to
  ride what they bought in every later ORQWA. Every release opens and rides every world file
  ever released, as before: never change what a released format means, drop or tighten a rule
  existing worlds rely on, or make the app read them differently. New features go in as optional
  extensions (`extensions_used`, glTF extensions, files under `ext/`), so earlier releases still
  open newer worlds without them. A new `world::FORMAT` needs the maintainer's agreement, and the
  app then keeps reading every earlier format. Test the change against the released fixtures
  (`core/fixtures/worlds/`); keep a fixture for each released format and feature.

## Rust conventions

- Errors: `thiserror` in library crates, `anyhow` only in binaries (`orqwa-cli`, gdext glue).
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
- User-facing guides live in the `wiki/` Git submodule, published on the GitHub wiki.
- **Keep the wiki focused on usage guides:** setup, controls, settings, rider workflows and
  troubleshooting. Update affected guides in the same session and PR when those user tasks
  change, including navigation and screenshots where needed. Do not add implementation
  explanations, scenery feature descriptions, rendering or map-data limitations, visual bug-fix
  notes, per-PR summaries or scenery regeneration notices. Visual feature additions and internal
  changes do not require wiki edits. Road surfaces, estimated widths and scenery generation
  belong in repository documentation and the PR, together with technical details, validation
  and review images. Do not invent a usage-guide section to satisfy a general docs-update rule.
  When editing the wiki:
  Run `make wiki-init` before reading or editing guides. Preserve local changes; never reset
  a dirty wiki checkout or edit on a detached HEAD: create/resume a documentation branch.
  Run `make wiki-prepare BRANCH=docs/<change>`, explicitly stage `git add wiki`, and include
  the wiki comparison link in the application PR. Never push to the published wiki branch:
  the workflow publishes only after the application change reaches `main`.
  See [the wiki workflow](docs/development.md#usage-wiki) for setup and recovery.
- **Update affected docs with the change, in the same PR.** Update only the documents whose
  subject changed; the wiki is affected only when rider usage instructions change, as scoped
  above. A change is not done until every affected doc says what the code now does: the usage
  wiki (`wiki/*.md`, only for usage changes), README,
  `docs/PLAN.md`, `docs/REQUIREMENTS.md` (append-only), ADR amendments, `docs/CREDITS.md`,
  `docs/OPEN-QUESTIONS.md` (remove what is settled), `art/README.md` and the skills in
  `.claude/skills/`.
- **Every server ORQWA contacts is listed** in the wiki's
  [When ORQWA needs the internet](https://github.com/ORQWA/ORQWA/wiki/Getting-started#when-orqwa-needs-the-internet)
  section, which riders use for privacy and firewall rules. Adding, changing or removing a
  network call updates that list in the same PR.
- **Images in docs stay current too.** Re-render those a change affects: app screenshots with
  `scripts/screenshots.sh`, the model galleries with `GALLERY=docs/images/models` and
  `scripts/render-models.sh` / `scripts/render-riders.sh`. Check wiki links as well as repository
  docs before deleting unused images. Wiki screenshots use absolute raw URLs to this repository.
  PR review images are committed under `docs/images/` and linked from the PR by commit
  (`raw.githubusercontent.com/<owner>/ORQWA/<commit>/…`), so pruning them breaks no PR.

## Git

- GitHub, trunk-based, short-lived branches, PRs, squash-merge.
- Conventional Commits (`feat:`, `fix:`, `docs:`, `refactor:`, `test:`, `chore:`, `ci:`).

## Working agreement for Claude

- Small PR-sized steps, one roadmap phase at a time; stop for review after each meaningful step.
- While working, `scripts/check.sh --quick` runs only the checks the changes can affect; never
  run checks and GPU renders at the same time.
- Before declaring done, run in the container: `cargo fmt --check`,
  `cargo clippy --all-targets -- -D warnings`, `cargo test`, `cargo deny check`, and `gdlint`
  (`scripts/check.sh` runs them all), run `make wiki-check`, and update the docs and their images
  (see Documentation). Check both repository statuses before declaring preparation complete.
- Keep the checkboxes in `docs/PLAN.md` up to date.
