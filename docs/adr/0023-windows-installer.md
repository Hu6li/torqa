# ADR 0023 — A Windows installer, and updates from within ORQWA

- Status: accepted
- Date: 2026-10-19

## Context

ORQWA for Windows ships as a zip holding `ORQWA.exe` and `orqwa_gd.dll`. A rider unpacks it
somewhere, finds it again to start it, and replaces both files by hand for every release. A
rider who wants the newest version for every real ride needs more than that: an install into
Program Files, a Start-menu entry, an uninstaller, and in the end ORQWA telling them about a new
version and installing it.

Options considered:

- **An MSI built with the WiX Toolset**: the standard Windows installer format, installed and
  removed like any other app, upgradable in place. Updates are whole installers (about 60 MB).
  WiX is the de-facto tool for building MSIs. It runs only on the Windows CI runner.
- **Velopack**: a `Setup.exe` into the rider's own folder with background delta updates, applied on
  restart. Smaller updates, but no MSI, and a Rust crate plus its own packaging tool.
- **MSIX**: Windows' newer package format, which cannot be installed unless it is signed.
- **The zip only, with an update notice**: no installer at all.

## Decision

- **Installer.** The `windows` CI job builds `ORQWA-windows-x86_64.msi` with **WiX v7** from
  `scripts/release/ORQWA.wxs`, next to the zip, and every release carries it.
  - **Install.** Per machine into `Program Files\ORQWA`, with an advertised Start-menu shortcut,
    the app's icon under Installed apps, and a fixed upgrade code. A newer MSI, or the same
    version again, replaces the installed one; an older one refuses to downgrade.
  - **Version.** The MSI's version is the `.exe`'s (`a.b.c.0`, kept in step with
    `core/Cargo.toml` by `scripts/release/version.py`).
  - **The rider's data stays.** It lives in `%APPDATA%` and `%LOCALAPPDATA%`, never in the
    installation, so installing, upgrading and uninstalling keep rides, riders and settings.
  - **Checked in CI.** The job installs the MSI silently, checks the files and the shortcut,
    and uninstalls it again.
- **WiX's terms.** WiX's source is under the Microsoft Reciprocal License. Since v7 its
  binaries come with the [Open Source Maintenance Fee EULA](https://github.com/wixtoolset/wix/blob/main/OSMFEULA.txt),
  which every build accepts (`wix build -acceptEula wix7`). The fee applies only to users that
  earn at least US$10,000 a year with it; ORQWA earns nothing. WiX builds the installer only,
  and none of it ships inside ORQWA.
- **Unsigned for now.** Like the `.exe`, the MSI is not code-signed, so Windows' SmartScreen
  warns once per new version ("More info → Run anyway"). Signing (SignPath for open-source
  projects, or Azure Trusted Signing) can follow without changing the installer.
- **Updates.** ORQWA will look for a newer release once at start-up, never during a
  ride, behind a setting that turns it off. It will offer to download the new MSI and install it
  over the running version. The servers it contacts will be added to the wiki's internet list
  with that change.

## Consequences

- Windows riders install, update and remove ORQWA like any other app, and their data survives
  all three.
- Releases carry one more asset, and the Windows job takes about a minute longer.
- The project depends on WiX's EULA terms staying free for projects without revenue. If they
  change, the installer can be built from WiX's source, which the EULA leaves free.
- macOS and Linux keep their archives for now.

## Amendment 2026-10-19: the update function

- **Where.** A crate of its own, `orqwa-update`: GitHub's latest release
  (`api.github.com/repos/ORQWA/ORQWA/releases/latest`), its version against the
  running one, and the installer asset downloaded and started. `orqwa-app` runs it in the
  background like its other online work, and the front end only shows what it finds.
- **When.** Once at start-up in released builds, unless this computer turned it off
  (`check_updates` in `computer.toml`, R78). Builds started from the editor, and the checks, never
  look on their own. Settings → About also checks on request. A check never starts during a ride,
  and a newer version found then is offered after the ride. At start-up only a newer version is
  told; a failure goes to the log. On request, being up to date and failures are told too.
- **Versions.** Tags are `v<major>.<minor>.<patch>`. A release counts only when its numbers are
  higher than the running version's. A pre-release, which is never GitHub's "latest" anyway, and
  a development build's version are never offered.
- **Installing (Windows).** The MSI is downloaded into the cache (`updates/ORQWA-<version>.msi`)
  and kept only when complete. ORQWA then starts `msiexec /i … /passive` through a hidden
  `cmd` that waits about three seconds, so ORQWA has quit before its files are replaced, and
  starts ORQWA again afterwards. ORQWA quits at once. Windows asks for permission, because the
  installation is per machine.
- **Elsewhere.** macOS and Linux have no installer yet. The offer opens the release page instead.
- **Servers.** `api.github.com` for the check, and `github.com` with
  `release-assets.githubusercontent.com` for the download, are listed in the wiki's internet list.
- **No new dependency.** It uses `reqwest`, `serde_json`, `tokio` and `thiserror`, which the
  workspace already has.

## Amendment 2026-10-22: versioned release files (#322)

- **Names.** Each release file carries its version: `ORQWA-0.5.2-windows-x86_64.msi`,
  `orqwa-cli-0.5.2-linux-arm64.tar.gz`. Downloads of different releases no longer overwrite each
  other or look alike. The release script adds the version; builds and CI artifacts keep plain
  names, so the build jobs are unchanged.
- **Updates.** The updater takes the installer named with the release's version, and the plain
  name only where that is missing. Every ORQWA before this change knows only the plain name, so
  releases also carry the installers under it (and 0.5.0's legacy name, ADR 0026) until riders
  have moved past those versions.

## Amendment 2026-10-26: only versioned names (R147)

- **Releases.** From 0.5.4 on, each file goes up only under its versioned name. 0.5.3 is the last
  release with the plain-named installers (`ORQWA-windows-x86_64.msi`,
  `ORQWA-android-arm64.apk`) and the legacy-named 0.5.0 installer.
- **Updates.** The updater takes only the installer named with the release's version: every
  release since 0.5.2 carries it, and ORQWA asks only for the latest. Releases 0.5.0 and 0.5.1
  no longer find an installer they know, so their update check opens the release page, as on
  macOS and Linux. The installer downloaded there still updates them in place.
