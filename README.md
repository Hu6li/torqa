<p align="center">
  <picture>
    <source media="(prefers-color-scheme: dark)" srcset="docs/brand/orqwa-lockup-dark.svg">
    <img src="docs/brand/orqwa-lockup-light.svg" alt="ORQWA" width="220">
  </picture>
</p>

<p align="center"><strong>Make waves with watts, not payments</strong></p>

<p align="center">Open-source · Free · Local-first · Indoor cycling</p>

<p align="center">
  <a href="https://github.com/ORQWA/ORQWA/actions/workflows/ci.yml?query=branch%3Amain"><img src="https://github.com/ORQWA/ORQWA/actions/workflows/ci.yml/badge.svg?branch=main" alt="CI"></a>
  <a href="LICENSE"><img src="https://img.shields.io/github/license/ORQWA/ORQWA" alt="License: GPL-3.0"></a>
  <a href="docs/development.md#builds"><img src="https://img.shields.io/badge/platforms-macOS%20%7C%20Windows%20%7C%20Linux%20%7C%20Android-blue" alt="Platforms: macOS, Windows, Linux, Android"></a>
  <img src="https://img.shields.io/badge/Rust-000000?logo=rust" alt="Rust">
  <img src="https://img.shields.io/badge/Godot-4-478CBF?logo=godotengine&logoColor=white" alt="Godot 4">
</p>

A modern, offline-first, open-source and free indoor cycling app. Import a GPX route or a ride video and
ride it on your smart trainer, through a generated 3D world or along the video, alone or with
friends.

<table>
  <tr>
    <td colspan="2" align="center">
      <img src="docs/images/ride-lake.jpg" alt="Riding along the Bielersee in ORQWA's 3D world"><br>
      <sub>Any GPX route as a 3D world, here along the Bielersee</sub>
    </td>
  </tr>
  <tr>
    <td colspan="2" align="center">
      <img src="docs/images/readme/group-ride.jpg" alt="A group ride with a friend in ORQWA's 3D world"><br>
      <sub>A group ride with a friend</sub>
    </td>
  </tr>
  <tr>
    <td colspan="2" align="center">
      <img src="docs/images/readme/video-course.jpg" alt="Riding a video course in ORQWA"><br>
      <sub>Along a real video, played at your speed</sub>
    </td>
  </tr>
  <tr>
    <td colspan="2" align="center">
      <img src="docs/images/readme/overlay.jpg" alt="ORQWA's compact overlay HUD"><br>
      <sub>Just your HUD, on top of any window</sub>
    </td>
  </tr>
</table>

<sub>Video: <a href="https://cycling.vangestel.online">“Le petit Menglonnais”</a> by Fred Van
Gestel, <a href="https://creativecommons.org/licenses/by-nc-sa/4.0/">CC BY-NC-SA 4.0</a>.</sub>

[More screenshots: app, workouts, cities, forests and route scenery](docs/screenshots.md).

> [!WARNING]
> **Written by AI, ridden at your own risk.** Every line of ORQWA is written by AI. It comes
> with no warranty of any kind, and we accept no liability for injury to you or for damage to
> your trainer, bike or any other gear (sections 15 and 16 of the [GPL-3.0](LICENSE)).

> [!NOTE]
> ORQWA is in early development: [docs/PLAN.md](docs/PLAN.md) shows what is done and what
> comes next.

## Features

<table>
  <tr>
    <td width="50%">🚴 <b><a href="https://github.com/ORQWA/ORQWA/wiki/Devices-and-riding#devices">Smart trainers</a></b><br>
      Bluetooth FTMS and Tacx FE-C trainers, heart-rate straps</td>
    <td width="50%">🗺️ <b><a href="https://github.com/ORQWA/ORQWA/wiki/Courses#import-a-course">Any GPX in 3D</a></b><br>
      A world from real terrain and OpenStreetMap, with seasons, weather and time of day</td>
  </tr>
  <tr>
    <td colspan="2">👥 <b><a href="https://github.com/ORQWA/ORQWA/wiki/Group-rides">Group rides</a></b><br>
      Up to eight riders with voice chat, peer to peer with no server in between</td>
  </tr>
  <tr>
    <td>🎬 <b><a href="https://github.com/ORQWA/ORQWA/wiki/Video-courses">Video courses</a></b><br>
      GoPro, Incyclist, Tacx RLV or any video with a GPX, played at your speed</td>
    <td>💪 <b><a href="https://github.com/ORQWA/ORQWA/wiki/Workouts">Workouts</a></b><br>
      ZWO, ERG, MRC and FIT, power or heart-rate targets, an editor and FTP tests</td>
  </tr>
  <tr>
    <td>⚙️ <b><a href="https://github.com/ORQWA/ORQWA/wiki/Devices-and-riding#virtual-gears">Virtual gears</a></b><br>
      24 gears, shifted from the keyboard, Shimano Di2, a Zwift Click or <a href="https://github.com/ORQWA/ORQWA/wiki/Devices-and-riding#remote">your phone</a></td>
    <td>🪟 <b><a href="https://github.com/ORQWA/ORQWA/wiki/Overlay">Overlay</a></b><br>
      Your HUD on top of any other window</td>
  </tr>
  <tr>
    <td>👻 <b><a href="https://github.com/ORQWA/ORQWA/wiki/Ghosts-and-pacers">Ghosts &amp; pacers</a></b><br>
      Your best ride, a friend's activity or a pacer at your FTP</td>
    <td>📈 <b><a href="https://github.com/ORQWA/ORQWA/wiki/Ride-history">History &amp; records</a></b><br>
      Power curve, fitness and records per course and climb, <a href="https://github.com/ORQWA/ORQWA/wiki/Uploading-rides">upload to intervals.icu and Strava</a></td>
  </tr>
</table>

📴 Works offline · ⚖️ realistic physics · 🎛️ customizable HUD ·
🔊 [wind and bike sound](https://github.com/ORQWA/ORQWA/wiki/Devices-and-riding#wind-and-bicycle-sound) · 🌍 English and German ·
🖥️ macOS, Windows, Linux and Android tablets

**Planned:** hardware video decoding, the overlay on Windows and Linux — see [docs/REQUIREMENTS.md](docs/REQUIREMENTS.md).

## Using ORQWA

Start with [Getting started](https://github.com/ORQWA/ORQWA/wiki/Getting-started); the
[usage wiki](https://github.com/ORQWA/ORQWA/wiki) covers every feature, from the
[command-line tool](https://github.com/ORQWA/ORQWA/wiki/Command-line-tool) to
[syncing between devices](https://github.com/ORQWA/ORQWA/wiki/Syncing-between-devices).

## Download

Tagged builds for macOS (Apple silicon), Windows, Linux and Android tablets are published on
the [Releases page](https://github.com/ORQWA/ORQWA/releases), with the app and CLI
archives attached. Between releases, download artifacts from the
[CI runs on `main`](https://github.com/ORQWA/ORQWA/actions/workflows/ci.yml?query=branch%3Amain)
while signed in to GitHub. [docs/development.md](docs/development.md#builds) explains the files.

## Contributing

Everything runs in a dev container: setup, checks and builds are in
[docs/development.md](docs/development.md). Conventions: [CLAUDE.md](CLAUDE.md); architecture
decisions: [docs/adr/](docs/adr/); third-party assets: [docs/CREDITS.md](docs/CREDITS.md).

## License

[GPL-3.0](LICENSE). The app's settings show, under About, the licence of everything built into it:
the Godot engine, FFmpeg, every Rust crate, and others' models and data. The video-course
screenshot shows a frame of a CC BY-NC-SA 4.0 video and is under that licence (see
[docs/CREDITS.md](docs/CREDITS.md)).
