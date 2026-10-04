# Open questions and findings

Things to review or decide together. Newest first; remove entries once settled.

## 2026-10-03 — Phase 5: video courses

### Needs your hardware

- **Video sound** (R26): please listen on a real ride. The sound is time-stretched to your
  speed with its pitch kept (WSOLA, 40 ms grains); tests check pitch and loudness, but not how
  it *sounds* — wind noise and voices may warble at very low speeds. It fades out below 0.2×
  the video's speed and is full from 0.5×; tell me if those feel wrong.
- **Video ride on the Mac**: please import a GoPro recording with GPS (or an Incyclist route
  video, choose its `.xml`) and ride it. Decoding runs on the CPU for now; 1080p should keep up
  on M1, 4K is scaled down per frame and may not. If it stutters, hardware decoding
  (VideoToolbox) and transcoding on import come next.
- **macOS CI**: the extension now links FFmpeg, built from source; the macOS job installs
  `nasm` for it. The first build takes a while (FFmpeg compile), later ones are cached.

### Findings

- **Route video libraries**: the free Van Gestel videos used by Incyclist are CC BY-NC-SA —
  fine to ride, not to bundle. Torqa only refers to videos; tests generate their own footage.
- **Plugin traits**: CLAUDE.md and the PLAN name `RouteImporter`, `TrainerDriver`, … but none
  exist yet — each capability has one implementation behind a concrete type. I noted in the
  PLAN that each trait comes with its second implementation; say if you want them earlier.

## 2026-10-03 — Phase 4: HUD editor, ghosts, audio

### Needs your ears / hardware (cannot be checked in the container)

- **Ambient sound**: removed after review — the synthesised sounds did not convince. If we
  want it back, it needs recorded sounds (CC0 sources) — a decision on an asset source.
- **Music control on macOS**: Torqa runs AppleScript (`osascript`) to Spotify, else Apple
  Music; macOS asks once for permission ("Torqa wants to control Spotify"). The usage text is
  in the export preset now. Please try M / . / , during a ride with Spotify or Music playing.
  Windows (media keys) and Linux (`playerctl`) are untested too.

- **German translation** (i18n PR): please read over `app/translations/de.po` — I used Swiss
  spelling (ss, "Velo") and informal "du". Error messages from the core (e.g. file or network
  errors) are still English only.

- **Reconnect at start** (R41): untested with real devices — please start Torqa with the KICKR
  awake and the strap on after riding with them once; both should show as connected without
  scanning. Device identifiers come from the system (address on Linux/Windows, a per-computer
  UUID on macOS); if one changes, the name is used as a fallback.

### Findings

- **Ride names in FIT** (R50): FIT has no activity-title field, so names live in the ride's
  JSON only; uploaders (Phase 7) pass the name as the title (Strava, intervals.icu accept it).
  If you also want it inside the FIT file, the sport-profile name would be the only place.

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
  hundreds of rides.

- **Profiles (PR #10)**: rides are now saved per rider in `profiles/<rider>/rides/`; rides
  saved earlier in `rides/` are not moved. Mass comes from the profile (rider + bike).
- **Lake Biel fixture** reports a steepest grade of 19 % — the elevation profile has a sharp
  spike around 2.3 km, probably a bridge/underpass the smoothing does not catch. Worth a look
  on a real ride.

- **Aerial imagery dropped** (PR #8): SWISSIMAGE draped on the terrain looked worse than the
  land-cover shading and was removed. PR #8 now only brings zoom 15 terrain and the
  first-person camera.
- **60 fps on M1** not yet measured (software renderer in the container only).
- **GitHub token** cannot read check results (`checks:read` missing), so I can't see CI status
  of PRs; I rely on `scripts/check.sh` locally.
