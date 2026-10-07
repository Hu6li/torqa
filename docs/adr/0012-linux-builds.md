# ADR 0012 — Linux builds on an older Ubuntu runner

- Status: accepted
- Date: 2026-10-07

## Context

Torqa is to run on Linux as well as macOS and Windows (R1). The dev container already builds
and tests everything on Linux, so the Linux app could be built there like the checks. But a
Linux binary needs the glibc it was linked against or newer: the container is Debian 13
(glibc 2.41), so its builds would not start on Ubuntu 24.04 LTS (2.39), Debian 12 or other
current long-term releases — most of the riders' machines.

Options considered:

- **Build in the dev container**: no exception to the containers-only rule, but the builds
  only run on the newest distributions.
- **A separate, older build container**: portable, but a second image to keep current, against
  "the most recent stable version of everything".
- **Build natively on an older GitHub Ubuntu runner**, as the macOS and Windows apps are built
  on their runners.
- **Flatpak / AppImage**: portable and with desktop integration, but a new toolchain and
  dependency; a later step if wanted.

## Decision

- The `linux` CI job builds the GDExtension, the CLI and the app natively on
  **`ubuntu-22.04`** (glibc 2.35) for x86_64 and **`ubuntu-22.04-arm`** for arm64, with the
  dev container's build packages (`libdbus-1-dev`, `pkg-config`, `libclang-dev`, `nasm`).
- The app is exported with Godot's Linux template, the PCK embedded, `libtorqa_gd.so` beside
  it, and packed as a tarball (artifacts drop the executable bit).
- The job starts the **exported** app headless and checks that it loads the extension and runs
  the main scene without errors, so a build that would not start is caught on the runner.
- Development, checks and tests stay in the dev container; the runner only produces releases.

## Consequences

- The builds run on distributions with glibc 2.35 or newer (Ubuntu 22.04+, Debian 12+, Fedora
  36+ and their peers); BlueZ (D-Bus) must run for Bluetooth, as on any desktop.
- When GitHub retires `ubuntu-22.04`, the job moves to the oldest runner left and the minimum
  glibc rises with it.
- No desktop integration yet (`.desktop` file, icon, menu entry): riders start `Torqa.x86_64`
  from the unpacked folder. Flatpak or AppImage can add this later.
