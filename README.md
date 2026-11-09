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

ORQWA is a free, open-source indoor cycling app. Import a GPX route or a ride video and ride it
on your smart trainer, through a 3D world or along the video, alone or with friends.

<table align="center">
  <tr>
    <td width="50%" align="center">
      <a href="docs/images/ride-lake.jpg"><img src="docs/images/ride-lake.jpg" alt="Riding along the Bielersee in ORQWA's 3D world" width="400"></a><br>
      <sub>A GPX route along the Bielersee in 3D</sub>
    </td>
    <td width="50%" align="center">
      <a href="docs/images/readme/group-ride.jpg"><img src="docs/images/readme/group-ride.jpg" alt="A group ride with a friend in ORQWA's 3D world" width="400"></a><br>
      <sub>A group ride with a friend</sub>
    </td>
  </tr>
  <tr>
    <td width="50%" align="center">
      <a href="docs/images/readme/video-course.jpg"><img src="docs/images/readme/video-course.jpg" alt="Riding a video course in ORQWA" width="400"></a><br>
      <sub>A ride video, played at your speed</sub>
    </td>
    <td width="50%" align="center">
      <a href="docs/images/readme/overlay.jpg"><img src="docs/images/readme/overlay.jpg" alt="ORQWA's ride figures in a compact overlay window" width="250"></a><br>
      <sub>Your ride figures over another window</sub>
    </td>
  </tr>
</table>

<sub>Video: <a href="https://cycling.vangestel.online">“Le petit Menglonnais”</a> by Fred Van
Gestel, <a href="https://creativecommons.org/licenses/by-nc-sa/4.0/">CC BY-NC-SA 4.0</a>.</sub>

[More screenshots](docs/screenshots.md).

> [!WARNING]
> **Written by AI, ridden at your own risk.** Every line of ORQWA is written by AI. It comes
> with no warranty of any kind, and we accept no liability for injury to you or for damage to
> your trainer, bike or any other gear (sections 15 and 16 of the [GPL-3.0](LICENSE)).

## Features

- **[3D routes](https://github.com/ORQWA/ORQWA/wiki/Courses#import-a-course):** Import a GPX route
  and ride through scenery based on real terrain and OpenStreetMap.
- **[Video rides](https://github.com/ORQWA/ORQWA/wiki/Video-courses):** Ride along with a video
  that plays at your speed.
- **[Continue another day](https://github.com/ORQWA/ORQWA/wiki/Courses#continue-another-day):**
  Save your place in a solo 3D or video course and continue with a fresh activity next time.
- **[Group rides](https://github.com/ORQWA/ORQWA/wiki/Group-rides):** Ride in a group of up to
  eight people, with voice chat, and choose the next ride together in the same lobby.
- **[Workouts](https://github.com/ORQWA/ORQWA/wiki/Workouts):** Import or create workouts,
  train to power or heart-rate targets, and take FTP tests.
- **[Trainers and sensors](https://github.com/ORQWA/ORQWA/wiki/Devices-and-riding#devices):**
  Connect supported Bluetooth trainers, power meters, cadence sensors and heart-rate straps.
- **[Virtual gears](https://github.com/ORQWA/ORQWA/wiki/Devices-and-riding#virtual-gears):**
  Shift with a keyboard, compatible shifter or your phone.
- **[Ghosts and pacers](https://github.com/ORQWA/ORQWA/wiki/Ghosts-and-pacers):** Ride against
  a previous activity or follow a pacer.
- **[Ride history](https://github.com/ORQWA/ORQWA/wiki/Ride-history):** Review your rides and
  records, and [upload activities to Strava or intervals.icu](https://github.com/ORQWA/ORQWA/wiki/Uploading-rides).
- **[Overlay](https://github.com/ORQWA/ORQWA/wiki/Overlay):** See your ride figures over
  other windows on macOS, Windows and Linux (X11), or in picture-in-picture on Android.
- **[Offline riding](https://github.com/ORQWA/ORQWA/wiki/Getting-started#when-orqwa-needs-the-internet):**
  Ride prepared courses without an internet connection.

## Download

Download ORQWA from the [Releases page](https://github.com/ORQWA/ORQWA/releases) for
macOS (Apple silicon), Windows, Linux or Android tablets.

For the latest development build, download an artifact from the
[CI runs on `main`](https://github.com/ORQWA/ORQWA/actions/workflows/ci.yml?query=branch%3Amain)
while signed in to GitHub. See [build details](docs/development.md#builds) for help choosing a file.

## Getting started

Follow the [getting-started guide](https://github.com/ORQWA/ORQWA/wiki/Getting-started) to set
up ORQWA and start riding. The [usage wiki](https://github.com/ORQWA/ORQWA/wiki) has guides
for devices, courses, workouts and more.

## Contributing

Everything runs in a dev container: setup, checks and builds are in
[docs/development.md](docs/development.md). Conventions: [CLAUDE.md](CLAUDE.md); architecture
decisions: [docs/adr/](docs/adr/); third-party assets: [docs/CREDITS.md](docs/CREDITS.md).

## License

[GPL-3.0](LICENSE). The app's settings show, under About, the licence of everything built into it:
the Godot engine, FFmpeg, every Rust crate, and others' models and data. The video-course
screenshot shows a frame of a CC BY-NC-SA 4.0 video and is under that licence (see
[docs/CREDITS.md](docs/CREDITS.md)).
