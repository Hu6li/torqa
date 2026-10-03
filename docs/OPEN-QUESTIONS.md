# Open questions and findings

Things to review or decide together. Newest first; remove entries once settled.

## 2026-10-03 — Phase 4: HUD editor, ghosts, audio

### Needs your ears / hardware (cannot be checked in the container)

- **Ambient sound** (audio PR): synthesised, so I could only test levels, not how it sounds.
  `AMBIENCE_DIR=/workspaces/torqa/screenshots godot --headless --path app -s
  res://tests/ui_smoke.gd` (in the container) writes the raw wind/road/rain/water/bird sounds
  as WAV files; in the app they are additionally filtered by speed. Too synthetic? Then we
  should add recorded CC0 sounds (a new asset source to decide on).
- **Music control on macOS**: Torqa runs AppleScript (`osascript`) to Spotify, else Apple
  Music; macOS asks once for permission ("Torqa wants to control Spotify"). The usage text is
  in the export preset now. Please try M / . / , during a ride with Spotify or Music playing.
  Windows (media keys) and Linux (`playerctl`) are untested too.
- **Video audio** (R26, original video sound) waits for video mode (Phase 5).

### Findings

- **Stacked PRs**: #19 (ghosts) builds on #18 (HUD editor), the audio PR on #19. All target
  `main`; merge in order and each shrinks to its own commit.

## 2026-10-03 — Course files (Phase 3c)

### Needs a decision

1. **Course files store inputs, not the pre-built world (deviation from ADR 0007).**
   A `.tqc` holds the GPX plus every terrain tile and map tile the course used. Opening it
   rebuilds route and world offline from those files — which also satisfies "rebuild with a
   newer generator" (R35) for free. Storing the pre-built world for an instant start needs a
   binary mesh format (own codec or a crate like `postcard`); I left it for a later step. OK,
   or do you want the stored world now? Measured: the 7 km Lake Biel course is 6.2 MB and
   opens offline on an empty machine in ~23 s with the debug build (release not measured).

### Findings / limits

- **History (PR #11)**: the history scans each rider's `rides/` folder and reads the JSON
  summaries; the SQLite index from ADR 0002 is not built yet — not needed until there are
  hundreds of rides or cross-ride queries (PRs, climbs).

- **Profiles (PR #10)**: rides are now saved per rider in `profiles/<rider>/rides/`; rides
  saved earlier in `rides/` are not moved. The setup screen's mass field is gone — mass comes
  from the profile (rider + bike). New dependency `toml` 1.1.6 (chosen in ADR 0004).
- **Lake Biel fixture** reports a steepest grade of 19 % — the elevation profile has a sharp
  spike around 2.3 km, probably a bridge/underpass the smoothing does not catch. Worth a look
  on a real ride.

- **Aerial imagery dropped** (PR #8): SWISSIMAGE draped on the terrain looked worse than the
  land-cover shading and was removed. PR #8 now only brings zoom 15 terrain and the
  first-person camera.
- **60 fps on M1** not yet measured (software renderer in the container only).
- **GitHub token** cannot read check results (`checks:read` missing), so I can't see CI status
  of PRs; I rely on `scripts/check.sh` locally.
