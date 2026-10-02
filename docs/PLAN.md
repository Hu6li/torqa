# Torqa — Architecture & Plan

See [REQUIREMENTS.md](REQUIREMENTS.md) for requirement IDs and [adr/](adr/) for decisions.

## Architecture

**Godot 4** (presentation, statically typed GDScript) + **Rust core** via GDExtension
([godot-rust/gdext](https://github.com/godot-rust/gdext)). All logic lives in Rust; Godot only
renders and presents. Rationale in [ADR 0001](adr/0001-stack-godot-rust.md).

```
core/                      Rust workspace (tokio + tracing)
  torqa-domain/            newtype units (SI internally), profiles, plugin traits
  torqa-physics/           speed integration, grade scaling, descent modes, virtual gears
  torqa-devices/           btleplug FTMS + HRM, fake trainer, (later) ANT+ FE-C, Click/OpenBikeControl
  torqa-routes/            GPX import, smoothing, DEM correction, climb detection
  torqa-world/             DEM/OSM download + tile cache, route-corridor terrain/road/feature data
  torqa-session/           ride loop (10–20 Hz), metrics (NP/TSS/zones), ghosts, recording
  torqa-storage/           data dir, FIT export, history index
  torqa-gd/                gdext bindings
  torqa-cli/               headless: scan devices, ride with fake trainer, export FIT
app/                       Godot 4 project: scenes/ride3d, scenes/hud, scenes/menus, shaders, i18n
docs/                      requirements, plan, ADRs
.devcontainer/             development container (all tooling lives here)
```

### Key design points

- **Plugin traits**: `RouteImporter`, `TrainerDriver`, `SensorDriver`, `ShiftInput`,
  `ActivityUploader`, `WorkoutParser`; Godot-side `RideMode` scenes (3D / Video / StreetView).
- **FTMS**: service `0x1826`, Indoor Bike Data `0x2AD2`, Control Point `0x2AD9`
  (Request Control `0x00`, Set Target Resistance `0x04`, Set Target Power `0x05`,
  Set Indoor Bike Simulation `0x11`). Heart rate: `0x180D` / `0x2A37`.
  Grade updates throttled to ~1–2 Hz.
- **Physics**: `P·η = v·(m·g·(Crr·cosθ + sinθ) + ½·ρ·CdA·v_rel²) + m·v·dv/dt`, integrated per
  tick. The trainer receives `grade × difficulty` (descents per R15).
- **Storage**: files are the source of truth; SQLite index is a local, rebuildable cache.
  See [ADR 0002](adr/0002-sync-safe-storage.md). Config TOML, data JSON
  ([ADR 0004](adr/0004-config-formats.md)).
- **3D world generation**: ~2–5 km corridor around the route; DEM → chunked heightmap meshes with
  LOD; road mesh from the smoothed route spline; OSM buildings (extruded), forests/fields
  (MultiMesh vegetation), water; procedural fill where data is missing; cached per route.
- **Video** (later): FFmpeg GDExtension (LGPL, hardware decode), GPMF parser, frame-blend shader.
- **Street imagery** (later): Street View Static API + Mapillary API v4, crossfade shader.
- **Development environment**: everything runs in Docker / devcontainer. Docker on macOS has no
  Bluetooth or Metal, so macOS builds come from GitHub Actions macOS runners and only the
  portable Godot editor and built Torqa app run natively.

## Roadmap

### Phase 0 — Foundation
- [x] Requirements, plan, ADRs, CLAUDE.md, README, LICENSE
- [x] `.devcontainer/` (Dockerfile + devcontainer.json) with Rust, cargo-deny, gdtoolkit, headless Godot
- [x] Rust workspace skeleton + lint config (clippy pedantic, missing_docs, cargo-deny)
- [x] Godot project + gdext hello-world
- [x] GitHub Actions: container job (fmt, clippy, test, deny) + macOS runner job (GDExtension + app export)
- **Exit:** macOS CI artifact runs on M1 and calls into Rust

### Phase 1 — Devices
- [x] FTMS driver (SIM / ERG / resistance), heart-rate sensor
- [x] Fake trainer
- [x] `torqa-cli scan` / `torqa-cli ride`
- [ ] macOS CLI artifact from CI
- **Exit:** CLI controls the KICKR Core 2 grade and reads power/cadence/HR

### Phase 2 — Route, physics, FIT
- [ ] GPX import, smoothing, online DEM correction
- [ ] Physics model, difficulty, descent modes
- [ ] FIT export
- **Exit:** headless fake-trainer ride over a GPX produces a valid FIT

### Phase 3 — 3D world (MVP)
- [ ] World generation (hybrid default): terrain, road, OSM features, vegetation
- [ ] Sky/weather presets, cameras, avatar
- [ ] Minimap, elevation profile, basic HUD
- **Exit:** ride a real GPX on the KICKR in 3D at 60 fps and save a FIT

### Phase 4 — Rider & history
- [ ] Multi-profile, zones, customizable HUD
- [ ] History & analysis, climbs/PRs
- [ ] Ghosts & pacers, audio, units, i18n

### Phase 5 — Video mode
- [ ] Import + transcode, GPMF + manual sync, variable speed + frame blending

### Phase 6 — Street imagery
- [ ] Google Street View (user key) + Mapillary with crossfades

### Phase 7 — Extras
- [ ] Virtual gears + Zwift Click / OpenBikeControl / keyboard
- [ ] ERG workouts (ZWO/ERG/MRC/FIT + editor), FTP test
- [ ] Uploaders, ANT+ FE-C, Windows/Linux builds, logo

## Risks

- Zwift Click protocol is reverse-engineered and may change → isolated behind `ShiftInput`.
- Garmin / TrainingPeaks / Komoot upload APIs need partner approval.
- Insta360 GPS extraction is less documented than GoPro GPMF.
- Free realistic terrain imagery is limited → procedural texturing by OSM landuse/slope by default.

## Verification

- **Unit tests**: FTMS packet encode/decode, physics vs reference values (e.g. 250 W, 75 kg, 0 %
  → ~36–37 km/h), GPX fixtures, smoothing, climb detection, NP/TSS.
- **Integration**: `torqa-cli ride --trainer fake --route fixtures/x.gpx` → FIT validated with the
  Garmin FIT SDK / `fitparse`, plus a test upload to intervals.icu.
- **Hardware**: ride on the KICKR Core 2 — resistance follows grade/difficulty, HR strap reads,
  reconnects after dropout.
- **Performance**: Godot profiler ≥ 60 fps on M1 on a long hilly route; world-generation time
  measured and cached.
