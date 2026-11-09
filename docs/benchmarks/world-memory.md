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

Before the chunks: the route, the map, the elevations and the course map take 170 MB. Building
the chunks takes it to 4.8 GB; the whole-world passes after them (barriers against pavement,
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
| World checksum | `a6a04ff670d2c2de` | `a6a04ff670d2c2de` |

- Meshes are built by pushing, so their arrays held two fifths more than they used; each chunk
  now gives that back when finished. Resident memory drops less than the room given back, as
  glibc keeps much of what is freed.
- Barriers were checked against a copy of every street and track triangle of the world (in
  `f64`, over 600 MB here). Bridges are now built first, so every barrier is known, and only
  the pavement in the cells their checks reach is gathered: the same answers, from a small
  fraction of the triangles.

## Still to do

Even so, the world alone is 3.2 GB, and Godot holds its own copy of every chunk it uploads,
never freeing one; the tablet ends ORQWA about halfway through building the chunks. Only
keeping the chunks near the rider bounds memory for any course length (PLAN Phase 12 G).
