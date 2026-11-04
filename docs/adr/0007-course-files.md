# ADR 0007 — Course files (`.oqc`)

- Status: accepted; amended 2026-10-03 (format 1 bundles inputs only, see below), 2026-10-15,
  2026-10-19 (a sample course), 2026-10-20 (worlds kept beside, ADR 0025), 2026-10-25 (map
  detail, ADR 0033)
- Date: 2026-10-02

## Context

Preparing a course downloads terrain and map data and builds a 3D world. Today the downloads
live in OS cache folders (which may be purged, hold far more than one course needs and cannot be
shared) and the world is rebuilt each time. Riders want to keep, pick and share prepared courses
offline (R32–R35).

## Decision

A course is a **zip container** with the extension **`.oqc`**:

```
manifest.json      format version, generator version, name, length, climbing, max grade,
                   created, source, attribution (OSM ODbL, terrain CC BY)
route.gpx          the original track
route.json         processed route points (position, elevation, distance, surface)
terrain.bin        corridor terrain heights (compressed)
map.json           OpenStreetMap features within the corridor only
world/             pre-built world: road, water, terrain chunks, buildings, trees (binary)
preview.png        image for the course list
```

- The **pre-built world is stored** so a course starts instantly on any machine. The inputs are
  stored too, so a course can be rebuilt when the world generator improves: the manifest's
  generator version tells whether the stored world is current.
- Files are written atomically and versioned; unknown newer format versions are rejected with a
  clear message rather than misread.
- The **library** is the `courses/` folder of the data directory; any `.oqc` placed there is
  listed. Import copies a file into it; export is a plain file copy.
- **Video courses** use the same format with sync data and a reference (name, size, hash) to the
  video file next to the course, not the video itself. Preparing a course puts the course's own
  copy of the video there (a hard link on one file system), so the course outlives the file it
  was imported from (#165).
- Sharing is file-based (mail, Nextcloud, USB, websites); no online catalog for now.

## Consequences

- Course files are larger than inputs alone: roughly 4–6 MB for a 2.4 km climb with a village,
  potentially a few hundred MB for long routes. Mesh quantisation can shrink this later.
- Attribution travels with the data, satisfying ODbL and CC BY when courses are shared.
- New dependency: `zip` (MIT) with only the pure-Rust deflate backend.

## Amendment 2026-10-03 — format 1

The first implemented format bundles the **inputs** and no pre-built world:

```
manifest.json      format, generator, name, length, climbing, max grade, created, attribution
route.gpx          the original track
data/…             every terrain tile and map tile the course was built from,
                   under its path relative to the download cache
preview.png        the course's map with its route, drawn when it was prepared (#192)
```

Data providers record which cached files they read or wrote while a course is prepared; saving
packs exactly those. Opening a course puts them back into the cache (existing files are kept;
cache paths are versioned) and builds the course offline. This needs no own format for terrain,
map or meshes, and every course is always built by the current generator (R35).

`preview.png` (added 2026-10-10, #192) is the picture the course's card shows: the flat map
the ride's minimap draws, with the route on it. The app draws it once the world is built and
puts it into the file; a course without one gets it the next time its world is built. It is
an optional entry, so the format stays 1.

Measured on the 7 km Lake Biel route: 6.2 MB, opened on an empty machine offline in ~23 s
(debug build). Storing the pre-built world for an instant start remains a later step
(format 2); readers reject formats newer than they know.

## Amendment 2026-10-13 — countries in the manifest (#230)

`manifest.json` gains an optional `countries` list: the countries the route passes, in order,
each with its ISO 3166-1 two-letter `code` and its `names` by language, so a shared course
names its countries offline (R68). The field is absent until the countries were looked up and
an empty list where none is known, so ORQWA fills in older courses' countries in the background
and never asks again for a course without any. Older ORQWA versions ignore the field; the
format stays 1. Filling it in rewrites the course file, like a rename: two machines doing so
before their sync folder catches up may leave a sync conflict copy (ADR 0002).

## Amendment 2026-10-14 — the course map's tiles in `data/` (#236)

The ride's minimap shows the land around the whole route (R70), as the course's picture does:
the picture's map tiles (ADR 0008, zoom 4–14 as the frame needs) are now recorded like the
corridor's and bundled under `data/`, so the map is made again, offline, whenever the course
opens. Measured on twelve real courses (12–212 km): 4–12 tiles, 64–890 KB before deflating
(about a quarter less after), 0.5–4 % of most course files and up to 18 % of the smallest (a
video course of 0.6 MB); making the map takes under 25 ms. Courses prepared before take their
tiles from the cache, else download them when opened; their files stay as they are. The
format stays 1.

## Amendment 2026-10-15 — no stored world

Format 1 (inputs only) is final: a stored pre-built world (format 2) is no longer planned.
Rebuilding keeps every course on the current generator; the wait before a ride is to be
hidden by building the world while the course's page is open (#237).

## Amendment 2026-10-19 — a sample course (R95)

A rider who has just downloaded ORQWA has no course to ride before importing one, which needs
the internet. ORQWA now ships one prepared course, **Twann – Chasseral** (20.4 km, 1,190 m of
climbing, 13.1 MB), kept in `core/orqwa-gd/courses/` with how it was made.

- It is an ordinary format 1 course, prepared in the app from its GPX like any import, so it
  rides offline from its own tiles and is rebuilt by the current generator when opened.
- The GDExtension embeds it (`include_bytes!`); the core does not, as the command-line tool,
  which shares the core, has no use for it. Every platform's export carries it with no extra
  files, and the macOS bundle needs nothing added after it is signed.
- On start, when the data directory has **no `courses/` yet**, the app writes the library with
  the sample in it. It is written to `courses.part/` and renamed, so a first start cut short
  leaves no empty library that would keep the sample out. An existing library is never
  touched: a rider who deleted the sample does not get it back, and riders updating ORQWA keep
  their library as it is.

Consequences: the repository and every app download grow by the file's 13 MB, and the
extension takes as much memory. The sample's tiles are those of when it was prepared; it is
prepared again only when a new course format makes it unreadable.

## Amendment 2026-10-20 — worlds kept on the computer (ADR 0025)

Building the world while the course's page is open (amendment of 2026-10-15) is replaced: a
computer may keep a course's built world in its cache (R101, [ADR 0025](0025-kept-worlds.md)),
opt-in, as a world is 10–40 times its course file. Format 1 stays final and the course file
unchanged. R35's promise to rebuild from the stored inputs holds: a kept world from an older
generator is ridden until the rider has it built again.

## Amendment 2026-10-25 — map detail in `data/` (ADR 0033)

A course prepared while the rider has a region's map detail carries the cells it read, as it
carries its tiles: `data/osm/detail-v1/<region>-<date of the data>/12/<x>/<y>.json`. The
manifest's optional `map_detail` names their extracts, and opening the course reads exactly
those, so it comes out the same on any computer, with the region downloaded or not. Format 1
stays: older ORQWA versions leave both unread. Opened in the library of a computer with a
region's detail ready, a course takes it and its file is rewritten with it (R144, ADR 0033).
Sharing or syncing that `.oqc` carries the map detail used; the receiving device needs neither
the full Geofabrik download nor a separate copy of its extracted region cache.
