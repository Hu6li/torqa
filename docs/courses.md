# Courses

A course is a prepared route saved as one `.tqc` file: the GPX track plus the terrain, map data
it needs. Courses ride **fully offline**, on any computer.

## Prepare a course

Import a GPX route with **Import route, video or course…** on the Courses tab while online.
Torqa downloads terrain and map data, builds the 3D world and adds the course to your library.

## Ride a course

Click its card on the Courses tab, then **Ride**. No internet connection is needed.

## Share a course

Course files are ordinary files: send them by mail, put them on Nextcloud, a USB stick or a
website. To use a course someone sent you, open the `.tqc` file with *Import route, video or course…*;
it is copied into your library.

The library is the `courses` folder of the Torqa data directory:

| System | Folder |
|---|---|
| macOS | `~/Library/Application Support/Torqa/courses` |
| Windows | `%APPDATA%\Torqa\courses` |
| Linux | `~/.local/share/torqa/courses` |

You can also copy `.tqc` files there directly. The data directory may live in a synced folder.

## Video courses

Courses ridden along a video refer to the video rather than containing it; keep the video next
to the `.tqc` file when moving or sharing them ([video.md](video.md)).

## Size and attribution

A course takes a few MB; sizes depend mostly on how much terrain and map data the
corridor around the route covers. Every course carries the credits of its data: © OpenFreeMap
© OpenMapTiles, data © OpenStreetMap contributors (ODbL); terrain by Mapterhorn (CC BY 4.0) and
AWS Terrain Tiles. Keep them when you share courses.

Courses saved by a newer Torqa may not open in an older one; update Torqa in that case.
