# Torqa — Requirements

Outcome of the initial requirements-elicitation session (2026-10-02, ~70 questions).
Requirement IDs (`R<n>`) are referenced from code, tests and ADRs.

## Vision

Torqa is a modern, **offline-first**, open-source indoor cycling app. Import a GPX route and ride it
on a smart trainer (primary: Wahoo KICKR Core 2) through a generated semi-realistic 3D world,
a synced ride video, or street-level imagery.

## General

| ID | Requirement |
|---|---|
| R1 | macOS (Apple Silicon M1+) first; Windows and Linux later. Stack must be cross-platform from day one. |
| R2 | Audience: the author + friends. Open source under **GPL-3.0**. **Zero budget**: free/open data only; users bring their own API keys for paid services. |
| R3 | Offline-first: a prepared ride works without internet. Online extras (DEM correction, world data download, Street View, uploads) are used when connected. |
| R4 | Extensibility is a core principle: interface-driven plugins for route importers, devices, shift inputs, uploaders, ride modes and workout parsers. |

## Devices

| ID | Requirement |
|---|---|
| R5 | Bluetooth LE **FTMS** (MVP). **ANT+ FE-C** later (no ANT+ dongle available for testing yet). |
| R6 | Direct-drive trainers first (KICKR Core 2). Architecture generic enough for any FTMS/FE-C trainer and smart bikes. |
| R7 | Sensors: BLE heart-rate strap. Zwift Click as shift input (no official protocol → reverse-engineered BLE, isolated and optional); also OpenBikeControl and keyboard/gamepad as shift inputs. |
| R8 | Trainer modes: **SIM** (slope simulation, MVP), **ERG** (target power), **resistance level**; free ride. |
| R9 | Drivetrain: real cassette (trainer handles shifting naturally) and Zwift Cog (app-side **virtual gears** — resistance offset via FTMS). Post-MVP. See ADR 0003. |
| R10 | Software **fake trainer** for development and automated tests. |

## Routes

| ID | Requirement |
|---|---|
| R11 | Import **GPX** (MVP); importer interface allows TCX/FIT/KML later. Routes are used as-is (no route editor). |
| R12 | Elevation: when online, correct with a terrain model (Mapterhorn, AWS Terrain Tiles fallback — ADR 0005) and smooth; offline, use cached tiles or GPX elevation with smoothing. |

## Physics

| ID | Requirement |
|---|---|
| R13 | Full physics model: rider + bike mass, CdA, Crr, gradient, air density; optional wind and drafting. |
| R14 | Trainer difficulty adjustable 0–100 % (scales the felt grade; virtual speed uses the real grade). |
| R15 | Descent behaviour configurable per ride: simulated coasting vs. clamp to flat. |

## Ride modes

| ID | Requirement |
|---|---|
| R16 | **3D world (MVP)**: semi-realistic style. World source user-selectable, default **hybrid** (real DEM + OpenStreetMap when available, procedural fallback). Weather/time-of-day presets. Cameras: first person, chase, drone — user's choice. Optional cadence-synced avatar. 60 fps on an M1 integrated GPU. |
| R17 | **Video**: own GoPro/Insta360 footage, downloaded videos, and plain videos without GPS (legacy Tacx RLV not supported). Auto-sync from embedded GPS (GoPro GPMF) with manual sync-point fallback. 1080p target; higher resolutions transcoded down on import. Variable playback speed with frame blending. |
| R18 | **Street imagery**: Google Street View (online only, user's API key, no caching per Google ToS) and Mapillary. Smooth crossfade/zoom transitions between panoramas. |
| R19 | Overlays: 2D minimap (OpenStreetMap tiles, cached for offline) and elevation profile with current position. |
| R20 | Ghosts / pacers, selectable per ride: own previous best, fixed W/kg or power pacer, ghost from a GPX/FIT activity. |

## Training & UX

| ID | Requirement |
|---|---|
| R21 | Structured workouts later, but architected for now: ZWO, ERG/MRC, FIT workouts, built-in editor. No multi-week training plans. |
| R22 | Rider profile: weight, FTP, max HR, power and HR zones, built-in FTP test. **Multiple user profiles** per installation. |
| R23 | Fully customizable HUD widgets: power (instant/3 s/10 s), cadence, HR, speed, distance, time, elevation gain, current & upcoming gradient, W/kg, NP, TSS, kJ, zone. |
| R24 | Metric and imperial units. English UI first, i18n-ready from day one. |
| R25 | Single window / fullscreen, external monitor / TV. Phone companion app is a possible future extension. |
| R26 | Audio, selectable per ride: ambient sounds, original video audio, music app control. |
| R27 | Auto-detected climbs with personal KOM-style times; personal records per route. Multiplayer possibly later — keep simulation state separable from rendering. |

## Data

| ID | Requirement |
|---|---|
| R28 | Export activities as **FIT** (MVP). |
| R29 | Uploaders behind a common interface: Strava, intervals.icu, Garmin Connect, TrainingPeaks, Komoot. (Garmin/TrainingPeaks/Komoot require partner approval → best effort.) |
| R30 | Local storage in a **configurable data directory** that may live in a synced folder (e.g. Nextcloud) → sync-safe design, see ADR 0002. |
| R31 | Detailed ride history and analysis: power/HR/cadence charts, zones, PRs. |

## MVP definition

Import a GPX → generate the 3D world → ride it on the KICKR Core 2 in SIM mode → save a FIT file.

## Test hardware available

KICKR Core 2, BLE heart-rate strap, Zwift Click + Cog. No ANT+ dongle.

## Open points

- Trademark / GitHub org availability check for "Torqa" before publishing.
- Logo: an orca riding a bike.
- License compatibility of BikeControl before reusing any Zwift Click protocol knowledge from it.
