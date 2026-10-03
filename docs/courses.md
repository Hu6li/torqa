# Courses

A course is a prepared route saved as one `.tqc` file: the GPX track plus the terrain, map data
it needs. Courses ride **fully offline**, on any computer.

## Save a course

1. Open a GPX route (*Open route or course…*) while online and wait for "3D world ready".
2. Press **Save as course**. The course lands in your library and appears under
   *Saved courses*.

## Ride a saved course

Pick it under *Saved courses*. No internet connection is needed.

## Share a course

Course files are ordinary files: send them by mail, put them on Nextcloud, a USB stick or a
website. To use a course someone sent you, open the `.tqc` file with *Open route or course…*;
it is copied into your library.

The library is the `courses` folder of the Torqa data directory:

| System | Folder |
|---|---|
| macOS | `~/Library/Application Support/Torqa/courses` |
| Windows | `%APPDATA%\Torqa\courses` |
| Linux | `~/.local/share/torqa/courses` |

You can also copy `.tqc` files there directly. The data directory may live in a synced folder.

## Size and attribution

A course takes a few MB; sizes depend mostly on how much terrain and map data the
corridor around the route covers. Every course carries the credits of its data: © OpenFreeMap
© OpenMapTiles, data © OpenStreetMap contributors (ODbL); terrain by Mapterhorn (CC BY 4.0) and
AWS Terrain Tiles. Keep them when you share courses.

Courses saved by a newer Torqa may not open in an older one; update Torqa in that case.
