# Open questions and findings

Things to review or decide together. Newest first; remove entries once settled.

## 2026-10-06 — Phases 7 and 10: workouts, overlay, extras

### Needs your hardware

- **Heart-rate hold on the KICKR** (#114, #118): Workouts → Heart-rate zone, with your strap.
  The controller is tuned on simulated hearts (settles in 5–10 min, no overshoot, at most
  30 W a minute); how much power a beat is worth comes from your FTP and maximum heart rate.
  Tell me if it swings around the target, takes too long, or ramps too fast for comfort.
- **Shimano Di2 shifting** (Di2 PR): built from the community's description of the D-Fly
  protocol (no Shimano spec, no code reused), untested on a real unit. Please assign two hood
  buttons to D-Fly channels in E-TUBE, scan in Devices & Settings, choose the shifter and its
  channels, and ride with virtual gears. The unit must be **paired**: macOS should ask the
  first time Torqa subscribes; if no presses arrive, pair it in the system Bluetooth settings.
  Tell me what a long press does on yours (I shift one gear per indication, so if the unit
  repeats while held it keeps shifting) and whether double presses come through.
- **Overlay on the Mac** (overlay PR): please try moving it by its bar, resizing it by the
  corner grip, clicking beside it (should reach the window below), and whether it stays on top
  of a browser playing a video while Torqa is not the active app. **Full view** should bring
  the window back as it was, full screen included. Allowing see-through windows is a project
  setting now; check that a normal 3D ride keeps its frame rate.

### Decisions I took (say if you want them otherwise)

- **Workouts on a course count for the course's records**: ERG holds the power, but it is
  still your power that moves you along. The summary and records treat them like any ride.
- **Workout names in the history** (e.g. "Heart-rate zone 3") are in the interface language
  of the moment the workout was ridden, like course names they are data, not translated later.
- **Overlay from course rides too** (R57): "button in ride/workout mode" read as any ride; a
  course ride in the overlay keeps following its gradient with the world hidden.

- **Structured workouts** (R21 PR):
  - ZWO cool-downs: files disagree whether `PowerLow` is the start or the lower value, so a
    warm-up always rises and a cool-down always falls, whichever comes first.
  - ERG files in watts stay in watts; they are not scaled to your FTP from the file's own
    `FTP =` line (Golden Cheetah can do either). MRC and ZWO follow your FTP.
  - FIT steps with a heart-rate target are ridden free (no power set); they could become
    heart-rate holds later. Steps by distance or the lap button are refused.
  - On a course, once the workout's last step is done you ride on in slope mode to the finish.
- **Workout editor**: messages are kept per step (at its start); a message in the middle of a
  step of an imported file moves to the step's start once the workout is edited and saved.
- **FTP test** (R22 PR): a ramp test (as Zwift's and TrainerRoad's) rather than 20 minutes all
  out — shorter and needs no pacing. It ends when the cadence stays below 50 rpm for 10 s; with
  the KICKR in ERG that is when your legs give way. Steps are 6 % of your FTP a minute.

- **Virtual gears** (gears PR): 24 gears from 0.75 to 5.5 (my own table, not Zwift's), 9 %
  apart; ↑ / ↓ shift. Tell me if the steps feel too big or small on the KICKR, or if you would
  rather have Zwift's own spacing.

### Needs a decision

- **Per-computer settings in the synced data folder**: the overlay's place and the graphics
  quality are per computer, but live in `settings.toml` of the data directory, which may be
  synced (R30). With two computers sharing it, the last one to save wins. Move them to a
  per-computer config file (next to the cache)?

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
