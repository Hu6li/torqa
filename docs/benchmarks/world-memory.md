# Course world memory

2026-11-09, dev container on x86-64 Linux (glibc), release build, offline from the course
file. These measurements explain why Android ended ORQWA on a Galaxy Tab S7 while it built a
long course; the tablet itself was not measured yet, and its allocator (Scudo) keeps freed
memory differently from glibc.

## What happened on the tablet

ORQWA 0.6.2, the course *Wada – Ōtarumi Pass* (57.1 km, 934 m of climbing, Japan; 683 chunks).
Android's record of why the app ended (`adb shell dumpsys activity exit-info org.orqwa.bike`)
shows it ended twice for low memory while on screen, at 2.6 and 2.5 GB resident, with no crash
log. The tablet has 5.5 GB, of which about 1.1 GB was available to apps.

## Measuring

`cargo run --release -p orqwa-app --example world_memory -- <course.oqc>` opens the course
offline, builds its world and prints the process's resident and peak memory at every tenth of
each loading step, a checksum of the whole world, the room its meshes hold unused, and its size
by part and by distance from the ridden road.

## Where the memory goes

Before the world: the route, the map, the elevations and the course map take 170 MB. The
world's whole-route preparation and the chunks take it to 4.8 GB (the preparation alone is
measured below, under *Built chunk by chunk*); the whole-world passes after them (barriers against pavement,
bridges) to the peak.

The world, by part (MB):

| Part | Size |
|---|---:|
| Streets around the route | 1,069 |
| Ground | 920 |
| Sidewalks, curbs and forecourts | 627 |
| Water | 198 |
| Buildings | 138 |
| Tracks | 83 |
| Trees and plants | 108 |
| Everything else | 103 |
| **World** | **3,246** |

Japan maps nearly every street, and each one within the 1.5 km corridor shapes the ground
around it in pieces of 2.7 m (1.3 m on hillsides), which the street is then cut along. By how far
a chunk's middle lies from the ridden road (MB):

| From the road | Chunks | Streets | Ground | Sidewalks | Tracks |
|---|---:|---:|---:|---:|---:|
| 0–250 m | 109 | 248 | 241 | 206 | 19 |
| 250–500 m | 104 | 162 | 144 | 109 | 12 |
| 500–750 m | 87 | 115 | 107 | 87 | 11 |
| 750–1,000 m | 95 | 136 | 117 | 71 | 16 |
| beyond | 288 | 409 | 311 | 154 | 25 |

Half the streets and ground lie beyond 750 m, where their fine pieces cannot be told apart.
Merging vertices identical in every attribute would save about a tenth: most that share a
position differ in normal or texture coordinate.

## Same world, less memory

| | Before | After |
|---|---:|---:|
| Meshes' unused room | 1,377 MB | 0 MB |
| After the chunks | 4.83 GB | 4.49 GB |
| **Peak** | **6.31 GB** | **5.18 GB** |
| Generating (three runs) | 95–100 s | 94–95 s |
| World checksum | `a6a04ff670d2c2de` | `a6a04ff670d2c2de` |

- Meshes are built by pushing, so their arrays held two fifths more than they used; each chunk
  now gives that back when finished. Resident memory drops less than the room given back, as
  glibc keeps much of what is freed.
- Barriers were checked against a copy of every street and track triangle of the world (in
  `f64`, over 600 MB here). Bridges are now built first, so every barrier is known, and only
  the pavement in the cells their checks reach is gathered: the same answers, from a small
  fraction of the triangles.

## Built chunk by chunk

2026-11-10, same container (8 cores, so 32 chunks a batch), release build. Each chunk is now
finished once its neighbours are built and handed on (`orqwa_world::generate_into`, ADR 0040's
step 2a); `generate` collects them into the same world. `world_memory <route.gpx> <cache>
--stream` builds a GPX route that way without the app and keeps only each chunk's hash. Wada's
route is the course's `route.gpx`, read offline from the cache the course was unpacked into.

| | Whole world | Streamed |
|---|---:|---:|
| Kirchenfeldbrücke, peak | 1.64 GB | 1.34 GB |
| Bielersee, peak | 1.43 GB | 1.00 GB |
| Oberalp, peak | 1.41 GB | 0.73 GB |
| **Wada, peak** | **4.78 GB** | **2.18 GB** |
| Wada, generating | 98 s | 100 s |

**The worlds are unchanged:** on all four courses the world's checksum is what it was before
the change (Wada as a course: `a6a04ff670d2c2de`), in a debug build too, where a chunk or
bridge reaching for ground or pavement it doesn't hold stops the build. A streamed build gives
the same chunk checksum as a whole one; both run the new code, so that shows only that
streaming drops nothing. The whole world's peak is lower than #541's 5.18 GB because the
pavement gathered for every barrier at once, and every chunk's barriers, are no longer held
together at the end.

**Where Wada's streamed peak goes.** The import (map and route) leaves 78 MB resident. Once
the whole-route preparation is done, before the first chunk, the process holds **1.16 GB**:
about 1.08 GB for the preparation (streets, the urban network, buildings and the rest of what
every chunk is built from, with the terrain tiles it read). The 170 MB "before the chunks"
above is the app's report before that preparation starts. The chunks around the build front
then take it to 1.71 GB after the first 32, 2.01 GB after 64, and 2.18 GB at most. ADR 0040
counts the preparation among the small parts; on Wada it alone is above R240's 1 GB for the
core while building, so it needs measuring by part, and shrinking, before that figure holds.

**What bounds the chunks held.** A chunk waits, whole, until all its neighbours are built, and
its ground and pavement stay until they are finished. Along one leg of a route that is a few
rows of chunks around the build front, and the batch adds `2 × cores` chunks (64 on 32 cores).
Where a later leg's corridor runs alongside an earlier one (out and back in a parallel valley,
stacked switchbacks, laps a few kilometres apart), the earlier leg's chunks on that side wait
for the later leg: chunks held then grow with the length of that seam. No build order avoids
it.

## Streets held once

2026-11-10, same container, release build, Wada built chunk by chunk as above, with a counting
allocator and a report after each step of the whole-route preparation. Before the first chunk
the preparation held 1,073 MB of live heap: 305 MB of decoded terrain tiles, all kept (now at
most 64 MB, `orqwa_terrain::TILE_BUDGET`), and a third in five indexes that each kept its own
copy of the streets' centre lines (13,207 streets, 865,151 points, 31 MB), in lists per cell
grown one item at a time. The streets are now held once (`streets::Shared`); the indexes keep
a street's and a point's number, 8 bytes, in one array sorted by cell (`cells::CellIndex`).

| Live heap (MB) | Before | After |
|---|---:|---:|
| Where plants keep off (`Clearance`) | 70 | 35 |
| Where channels stop at streets (`Channels`) | 51 | 14 |
| Formed street beds (`streets::Levels`) | 108 | 30 |
| Streets under bridges (`Below`) | 79 | 21 |
| Sidewalk ways, with the building sites | 99 | 35 |
| **Preparation** | **1,073** | **792** |
| Streamed peak | 2.25 GB | 2.01 GB |
| Generating | 98–110 s | 102 s |

With the tile cache as well, the preparation holds 555 MB live (676 MB resident) and the
streamed peak is 1.73–1.75 GB, in 99–102 s.

The worlds are unchanged: the four courses' checksums are main's (Kirchenfeldbrücke
`d809883d0cce28ce`, Bielersee `4c70f2da7b782cb9`, Oberalp `5d69f358523b43ef`, Wada as a course
`a6a04ff670d2c2de`), and on top of the tile cache the GPX routes' whole and streamed checksums
are as before (Wada's chunk checksum `11713ce55c887625`).

On the Tab S7, Wada streamed offline at the 64 MB tile budget, two runs each alternating with
the same build without this change:

| Tab S7 | Without | With |
|---|---:|---:|
| Resident after the preparation | 0.90–0.91 GB | 0.67 GB |
| Peak | 1.71–1.73 GB | 1.48–1.49 GB |
| Lowest memory available to the tablet | 0.90–1.00 GB | 1.23–1.31 GB |
| Generating | 342–345 s | 341–350 s |

The chunk checksum is `a0ca70babfcb9de9` in every run, with and without the change (the tablet's
build has a checksum of its own, as before).

## Sidewalks held compactly

2026-11-10, measured the same way, on top of the streets held once and the tile cache. Of the
555 MB the preparation then held, the urban surfaces' last step (cutting each sidewalk patch
free of zones, carriageways, junction fans and buildings) added the most: 307,223 patches at
102 MB (a third of it a list's spare room), the 361,033 polygons left after cutting at 59 MB
(668,256 small lists, two thirds of their room spare), and the junction fans' 482,190
triangles at 34 MB. The cutting also left its freed lists scattered through the heap, so the
process stayed larger than what it held.

Patches are quadrilaterals and junction fans triangles: each now holds its corners itself,
rather than in a list of its own. The polygons left after cutting go into one store, points in
one array (`urban::Shapes`), which the passes that follow (merging overlapping islands,
dropping walks too short to keep) rebuild in turn rather than edit in place. A cut whose bounds
miss a polygon keeps the polygon rather than copying it.

| Container | Before | After |
|---|---:|---:|
| Sidewalk patches | 102 MB | 60 MB |
| Polygons left after cutting | 59 MB | 28 MB |
| Junction fans | 34 MB | 24 MB |
| **Preparation**, live | **555 MB** | **472 MB** |
| Preparation, resident | 666–676 MB | 536–554 MB |
| Streamed peak | 1.74–1.75 GB | 1.60–1.63 GB |
| Generating | 99–102 s | 98–101 s |

The checksums are the same as above (the four courses whole, and the GPX routes streamed).

| Tab S7 | Without | With |
|---|---:|---:|
| Resident after the preparation | 0.58 GB | 0.50 GB |
| Peak | 1.48–1.49 GB | 1.41–1.42 GB |
| Lowest memory available to the tablet | 1.03–1.15 GB | 1.17–1.26 GB |
| Generating | 336–343 s | 338–342 s |

Two runs each, alternating; the chunk checksum is `a0ca70babfcb9de9` in every run.

## Smaller batches

2026-11-10, measured the same way, with a report of live heap before, during and after each
batch of chunks. The preparation ends at 472 MB live, but the streamed peak comes a gigabyte
later, while the chunks are built. At four chunks per core (32 a batch), between batches up
to 450 MB more is held: ground meshes and pavement kept for the neighbours of about 80 built
chunks (110–135 and 100–130 MB), some 40 chunks waiting for theirs, and the bridges built. A
batch's chunks are all built before any is finished, and add to that: 516 MB in the first
batch (472 → 988 MB live) and 379 MB in the second (876 → 1,255 MB), the highest live heap
of the build. The 130 MB held after the streets' lines are drawn, which the earlier report by
part counted as one step, is the tile cache filling to its budget there (64 MB) and the
streets themselves (31 MB).

A batch is now two chunks per core instead of four (16 on the container's and the Tab S7's 8).
The order chunks are built in, and so the world, stays the same; the cores are less evenly
busy towards each batch's end.

| Container | Four per core | Two per core | One per core |
|---|---:|---:|---:|
| Live between batches, highest | 925 MB | 970 MB | 970 MB |
| Live after a batch is built, highest | 1,255 MB | 1,086 MB | 1,035 MB |
| Streamed peak (resident) | 1.60–1.61 GB | 1.38–1.44 GB | 1.34–1.45 GB |
| Generating | 100 s | 112 s | 139 s |

From two per core on, the batch adds about 100 MB or less: the floor is what is held between
batches, the preparation and the chunks kept for their neighbours, with about 350 MB more
that the allocator keeps from it. One per core is no lower than two beyond the runs' spread,
and a quarter slower. The checksums are the same as above (the four courses whole, and the
GPX routes streamed).

| Tab S7 | Four per core | Two per core |
|---|---:|---:|
| Peak | 1.41–1.43 GB | 1.21 GB |
| Lowest memory available to the tablet | 1.14–1.18 GB | 1.29–1.30 GB |
| Generating | 333–341 s | 369–378 s |

Two runs each, alternating; the chunk checksum is `a0ca70babfcb9de9` in every run.

## Still to do

Even so, the world alone is 3.2 GB, and Godot holds its own copy of every chunk it uploads,
never freeing one; the tablet ends ORQWA about halfway through building the chunks. Only
keeping the chunks near the rider bounds memory for any course length (PLAN Phase 12 G).
