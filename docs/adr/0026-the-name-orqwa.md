# ADR 0026 — The name ORQWA

- Status: accepted
- Date: 2026-10-20

## Context

The previous name is already registered by another app, so the project needs a name of its own. Riders
of version 0.5.0 have their profiles, rides and courses in folders named after it, and Windows
riders an installation and an update check that names the old repository and installer.

## Decision

The app is **ORQWA**, orca, torque and watt (R102):

- **Spelling.** ORQWA in capitals wherever a name shows or names a product file: the window,
  About, docs, wiki, `ORQWA.exe`, `ORQWA.app` and the release files. Code follows its
  languages' conventions: the crates `orqwa-*`, `orqwa_domain`, `OrqwaApp`, `ORQWA_*`
  variables.
- **Brand.** The orca on a chainring stays the mark. The wordmark is new: rounded capitals with
  the mark's blue crescent inside the Q. `scripts/brand/wordmark.py` draws it and the lockups
  in `docs/brand/` from measurements of the design.
  The wiki's welcome and naming story also use the cycling-orca illustration supplied by
  the project owner (`docs/images/cycling-orca.png`, credited in `docs/CREDITS.md`).
- **Repository.** `ORQWA/ORQWA` on GitHub.
- **Riders' data.** The data folder is `ORQWA` on macOS and Windows and `orqwa` on Linux;
  the cache folder is `orqwa`. The old folder migration has been retired (#501).
  Existing ride files keep their names; new ones are `orqwa-<date>-<time>.fit`.
- **Identifiers.** The macOS bundle and the Android package are `org.orqwa.bike`. No APK was
  released under the old one.
- **Windows.** The installer keeps its UpgradeCode across releases so updates replace the
  previous installation in place.
- **Course files.** `.oqc` means ORQWA course (#501). The archive format stays unchanged.
  A temporary module migrates existing library files at startup without overwriting current
  courses; it and its startup call can be removed once riders have updated.
- **Android signing.** Read the existing key's alias from the release keystore, preserving
  its signing identity without a historical name in the build script.
- **Docs.** The name changes in every doc, earlier ADRs and existing requirements included. It
  is a name, not a change to what they decide or require, so the requirements stay
  append-only in substance.

## Consequences

- The app, repository, product files and current course extension use ORQWA's name.
- Existing course archives and their input data need no conversion.
- Windows upgrades and Android signing keep their existing identities.
