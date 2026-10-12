<p align="center">
  <picture>
    <source media="(prefers-color-scheme: dark)" srcset="docs/brand/torqa-lockup-dark.svg">
    <img src="docs/brand/torqa-lockup-light.svg" alt="Torqa" width="220">
  </picture>
</p>

<p align="center"><em>Killer watts. No cloud.</em></p>

<p align="center">
  <a href="https://github.com/bossm8/Torqa/actions/workflows/ci.yml?query=branch%3Amain"><img src="https://github.com/bossm8/Torqa/actions/workflows/ci.yml/badge.svg?branch=main" alt="CI"></a>
  <a href="LICENSE"><img src="https://img.shields.io/github/license/bossm8/Torqa" alt="License: GPL-3.0"></a>
  <a href="docs/development.md#builds"><img src="https://img.shields.io/badge/platforms-macOS%20%7C%20Windows%20%7C%20Linux-blue" alt="Platforms: macOS, Windows, Linux"></a>
  <img src="https://img.shields.io/badge/Rust-000000?logo=rust" alt="Rust">
  <img src="https://img.shields.io/badge/Godot-4-478CBF?logo=godotengine&logoColor=white" alt="Godot 4">
</p>

# Torqa

A modern, offline-first, open-source indoor cycling app. Import a GPX route or a ride video and
ride it on your smart trainer, through a generated 3D world or along the video.

<table>
  <tr>
    <td colspan="2" align="center">
      <img src="docs/images/ride-lake.jpg" alt="Riding along the Bielersee in Torqa's 3D world"><br>
      <sub>Any GPX route as a 3D world, here along the Bielersee</sub>
    </td>
  </tr>
  <tr>
    <td width="50%" align="center">
      <img src="docs/images/readme/video-course.jpg" alt="Riding a video course in Torqa"><br>
      <sub>Along a real video, played at your speed</sub>
    </td>
    <td width="50%" align="center">
      <img src="docs/images/readme/overlay.jpg" alt="Torqa's overlay on top of a browser window"><br>
      <sub>Just your HUD, on top of any window</sub>
    </td>
  </tr>
</table>

<sub>Video: <a href="https://cycling.vangestel.online">“Le petit Menglonnais”</a> by Fred Van
Gestel, <a href="https://creativecommons.org/licenses/by-nc-sa/4.0/">CC BY-NC-SA 4.0</a>.</sub>

> [!WARNING]
> **Written by AI, ridden at your own risk.** Every line of Torqa is written by AI. It comes
> with no warranty of any kind, and we accept no liability for injury to you or for damage to
> your trainer, bike or any other gear (sections 15 and 16 of the [GPL-3.0](LICENSE)).

> [!NOTE]
> Torqa is in early development: [docs/PLAN.md](docs/PLAN.md) shows what is done and what
> comes next.

## Features

<table>
  <tr>
    <td width="50%">🚴 <b><a href="docs/riding.md#devices">Smart trainers</a></b><br>
      Bluetooth FTMS (SIM, ERG, resistance) and heart-rate straps; your devices reconnect at start</td>
    <td width="50%">🗺️ <b><a href="docs/riding.md#the-3d-world">Any GPX in 3D</a></b><br>
      A stylized world from real terrain and OpenStreetMap, with weather and time of day</td>
  </tr>
  <tr>
    <td>🎬 <b><a href="docs/video.md">Video courses</a></b><br>
      GoPro, Incyclist, Tacx RLV or any video matched to a GPX; it plays at your speed</td>
    <td>💪 <b><a href="docs/workouts.md">Workouts</a></b><br>
      Hold a power or a heart rate, ZWO / ERG / MRC / FIT with an editor, three FTP tests</td>
  </tr>
  <tr>
    <td>⚙️ <b><a href="docs/riding.md#virtual-gears">Virtual gears</a></b><br>
      24 gears on a single cog, shifted from the keyboard or Shimano Di2 hood buttons</td>
    <td>🪟 <b><a href="docs/overlay.md">Overlay</a></b><br>
      Just your HUD, on top of a video or any other window</td>
  </tr>
  <tr>
    <td>👻 <b><a href="docs/ghosts.md">Ghosts &amp; pacers</a></b><br>
      Race your best ride, a friend's activity or a pacer at your FTP</td>
    <td>📈 <b><a href="docs/history.md">History &amp; records</a></b><br>
      Ride analysis, personal records per course and climb, FIT export and
      <a href="docs/uploads.md">upload to intervals.icu</a></td>
  </tr>
</table>

📴 Courses ride offline on any computer · ⚖️ realistic physics, adjustable difficulty ·
🎛️ a HUD your way · 🌍 English and German · 🖥️ macOS, Windows and Linux

**Planned:** Street View / Mapillary rides, uploads to Strava, ANT+ FE-C, hardware video
decoding — see [docs/REQUIREMENTS.md](docs/REQUIREMENTS.md).

## Using Torqa

| Topic | Doc |
|---|---|
| Start page, course gallery and course pages | [docs/start.md](docs/start.md) |
| Courses: offline riding and sharing | [docs/courses.md](docs/courses.md) |
| Video courses | [docs/video.md](docs/video.md) |
| Devices, riding and in-ride settings | [docs/riding.md](docs/riding.md) |
| Riders, zones | [docs/riders.md](docs/riders.md) |
| Ride HUD | [docs/hud.md](docs/hud.md) |
| Workouts and FTP tests | [docs/workouts.md](docs/workouts.md) |
| Overlay over other apps | [docs/overlay.md](docs/overlay.md) |
| Uploading rides to intervals.icu | [docs/uploads.md](docs/uploads.md) |
| Ghosts and pacers | [docs/ghosts.md](docs/ghosts.md) |
| Ride history, climbs and records | [docs/history.md](docs/history.md) |
| Music control | [docs/audio.md](docs/audio.md) |
| Translations | [docs/translating.md](docs/translating.md) |
| Command-line tool for testing trainers | [docs/cli.md](docs/cli.md) |

## Download

There are no releases yet. The builds for macOS (Apple silicon), Windows and Linux are
artifacts of the [CI runs on `main`](https://github.com/bossm8/Torqa/actions/workflows/ci.yml?query=branch%3Amain)
(download them while signed in to GitHub); [docs/development.md](docs/development.md#builds)
says which file is which.

## Contributing

Everything runs in a dev container: setup, checks and builds are in
[docs/development.md](docs/development.md). Conventions: [CLAUDE.md](CLAUDE.md); architecture
decisions: [docs/adr/](docs/adr/); third-party assets: [docs/CREDITS.md](docs/CREDITS.md).

## License

[GPL-3.0](LICENSE). The video-course screenshot shows a frame of a CC BY-NC-SA 4.0 video and is
under that licence (see [docs/CREDITS.md](docs/CREDITS.md)).
