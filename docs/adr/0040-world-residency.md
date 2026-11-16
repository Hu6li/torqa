# ADR 0040 — Only the rider's surroundings in memory: worlds built into a store, generated ahead

- Status: accepted
- Date: 2026-11-10

## Context

On a Galaxy Tab S7 (5.5 GB RAM), Android's low-memory killer ends ORQWA at about 2.5 GB while
it builds *Wada – Ōtarumi Pass* (57 km, Japan, 683 chunks). Nothing is reported to the rider:
the course doesn't load and ORQWA is gone (#542). Memory grows with the route's length and
how densely its corridor is mapped. The graphics preset makes no difference.

- **Core.** `orqwa_world::generate` builds every chunk of the corridor into `World::chunks`.
  Several passes then run over the whole world: barriers are checked against pavement,
  roadside protection against the neighbouring ground, and structures stand on the ground.
  `App` keeps the finished `World` for the ride. On Wada the finished world is 3.2 GB in the
  container, and the peak was 6.3 GB on `main` and 5.2 GB with #541. On the tablet, #541's
  build completes only as an `adb` process that Android cannot kill, at about 4.3 GB counting
  zram swap.
- **Godot.** `world_streaming.gd` uploads the chunks around the rider first (R224), then every
  other chunk, and never frees one. On a tablet, GPU memory is the same RAM.
- **What a ride needs.** Only chunks within the drawn range: 4.5 km for ground and streets
  times the preset's distance (0.65 on Low), plus the 1 km upload margin. Once a chunk is
  uploaded, the core reads it again only for a season change. Every other reader of all the
  chunks needs much less: the winter ground mask needs only their bounds, and the render
  tools and the benchmark can read the chunks one by one.
- **Neighbours only, after #541.** On `main`, every barrier is checked against an `f64` copy
  of every street and track triangle in the world. #541 builds structures before the pavement
  is gathered, so every barrier is known first. `pavement::Asked` then collects the 16 m cells
  their checks reach, `Pavement::near` keeps only those, and a `debug_assert` stops any check
  outside them. With that, the passes after the chunk loop read only near neighbours:
  - Protection heights correct only snow poles and delineators standing outside their own
    chunk's square. Each looks up the ground of the chunk whose square holds it.
  - A barrier or fence is checked only against pavement in the cells it reaches.
  - A structure's foundations read the ground along its run and under its piers. Which
    structures exist and who owns them (`Below`, rail bundles) comes from the road and the map
    before the chunk loop. Tunnels read the elevation model, not chunks.

  The ground mesh is final when its chunk is built. Later passes change only streets,
  tracks, structures and props.
- **Kept worlds** (ADR 0025) already store a zip of `whole` and `chunks/<n>`. Each chunk's
  entry begins with its centre, a length (`u32`, 3) and three `f32`. Readers find chunks by
  name, so the entries' order in the archive doesn't matter. Riders who don't keep worlds have
  no store.

Options considered:

- **Keep chunks deflated in memory:** about 2.4 times smaller (about 1.3 GB on Wada), but
  memory still grows with the route's length.
- **Build far chunks again when needed:** keeps the map, elevations and preparation in memory,
  and a tablet builds 5 to 10 times slower than it reads, using the CPU it needs for drawing.
- **Store uncompressed:** saves CPU, but takes 3.2 GB of disk for Wada.
- **Thin the far world** (coarser far streets, compact vertices, no urban mesh far away): each
  is worth doing (#542), but each only scales the curve down, and two of them change the
  world.
- **Write each finished chunk to a store on disk and keep only what is drawn:** memory then
  follows the surroundings and the preset. Only the parts built whole, the chunk index and the
  whole-route preparation still grow with the course, and those are small.

## Decision

The memory for building and riding a world is bounded by the rider's surroundings and the
graphics preset, plus the small parts that grow with the route (R240). Every step leaves the
built world byte-identical, so neither `GENERATOR` nor `stored::FORMAT` is raised for this.

### One store, always

Every world is built into a store in the cache directory, never the data directory (R30).
2a builds only worlds the rider keeps into a store, as nothing reads it yet. From 2b, when
the core reads chunks from the store, every world is.

- While the world is being built, each finished chunk is written to
  `<cache>/worlds/<fingerprint>.<pid>-<n>.building/chunks/<n>`. Each build claims its own
  directory (the next `<n>` if one is there), so two builds of one course, one stopped while
  the next begins, never share files. Process ids repeat after a crash and across sandboxes,
  so a directory is claimed only if it is not already there. Each file is a zip with one
  entry, already named `chunks/<n>`, deflated at level 1 as kept worlds are, on the core that
  finished the chunk. Each chunk can be read as soon as it is written, while others are still
  being built. A chunk is written under a temporary name and renamed when complete, as
  `KeptWorlds::write` does, so a chunk file that exists is whole, also after a build was
  stopped or killed. Chunk files are not synced to the disk: a store never outlives its
  build, and the page cache outlives a process that is killed. Only the kept world is synced.
- When building completes and the rider keeps worlds, the chunks are packed into the kept
  `<fingerprint>.world` with the `zip` crate's `ZipWriter::raw_copy_file`, in index order, so
  the kept file doesn't depend on build order. Nothing is inflated or deflated again, and no
  dependency is added. Raw copies carry their CRC unchecked, so a damaged chunk file shows only
  when the kept world is read, which ADR 0025 already handles. The building directory is then
  deleted. The kept world's layout stays as ADR 0025 describes it.
- A store belongs to the build that made it, and is deleted when that build lets go of it:
  once kept, when the build is stopped, or when a chunk fails to be written (the disk may be
  full). When the rider doesn't keep worlds, the store is the ride's, deleted when the course
  is closed or another course is opened.
- A store left by an ORQWA that stopped (a crash, a low-memory kill) is deleted by the next
  tidy of the kept worlds once it hasn't grown for 10 minutes, as a half-written kept world
  is. Age is only needed for other processes' stores: from 2b, ORQWA knows its own live
  builds and never tidies them, however long their preparation takes.
- **Size.** A store is the world deflated: ADR 0025 measured 10 MB per kilometre on Swiss
  courses. A densely mapped course is about twice that: Wada's store is 1,149 MB, 20.1 MB per
  kilometre (measured in 2a), and packing it into the kept world takes about 2 s. So every
  ride of a course that isn't kept writes that much to flash.
- **When the store can't be written** (disk full, unwritable cache):
  - On desktops, the chunks stay in memory as they do today, with a warning in the log.
  - On Android, that fallback is the low-memory kill again. So **Ride** stops before building
    and tells the rider how much free storage the course needs. The check allows 25 MB per
    kilometre, above the densest course measured.
- Android may clear the cache under storage pressure while a course is open. A missing chunk
  is handled like a damaged one (see Consequences).

### Chunk index without a format change

A chunk's centre is the first field of its entry. Reading a kept world therefore inflates only
16 bytes per entry to index it: the length and the three floats. Chunk `n` keeps its index in
the corridor's cell order (`chunks_near_route`, the order of `World::chunks` today), whatever
order chunks are built in.

### Generation in stages (2a)

Each chunk goes through these stages:

1. **Built:** `Corridor::chunk` has returned. Its *as-built pavement* is extracted right
   away: the street and track triangles `Pavement::near` would gather from it. `clear` later
   removes barrier triangles and rewrites the meshes, and a track's index count before shore
   gravel is stale after that. Today every chunk's pavement is gathered before any is
   cleared, so a neighbour must see the as-built copy.
2. **Finished:** its 3×3 neighbours are built. Its protection heights are corrected, its
   barriers and fences are cleared against the as-built pavement of the cells they reach, and
   it is written to the store. A chunk's content never depends on structures.
3. **Dropped:** its ground mesh and as-built pavement stay in memory until all eight
   neighbours are finished and every structure over it is built and cleared. Then they go. A
   finished chunk's other meshes go as soon as it is written.

Memory while building is therefore the whole-route preparation plus **two rings** of chunks
around the build front: built chunks waiting for neighbours, and finished chunks whose ground
and pavement neighbours still need.

Order rules, so the output stays byte-identical:

- `foundations::Ground` looks up chunks by cell. `Ground::at` answers with the first chunk in
  `World::chunks` order whose triangles hold the point, and on a shared edge two can. So the
  by-cell lookup tries its candidates in index order.
- **Structures** are built once the chunks under them are built. Each is cleared against the
  as-built pavement of the cells its barriers reach. `World::structures` is one mesh, with
  barrier ranges indexing into it. So the pieces are concatenated in `build_all`'s order (own
  runs, then rail bundles by length), whatever order they were built in.
- Chunks are built in route order: a cell's rank is the first route distance that reaches it.
  Neighbours are then built close together, and the two rings stay small. Cell order would hold
  a whole column of the corridor.
- `generate()` stays as a sink that collects the full `World`. The tests, the CLI and the
  checksums keep working on whole worlds.
- Each chunk asks for its pavement cells as #541's `pavement::Asked` does for the whole world,
  so the `debug_assert` keeps catching a check that reaches beyond them.

**The gate for 2a:**

- The same `world_memory` checksums as before the change, on the `kirchenfeldbruecke`,
  `bielersee` and `oberalp` fixtures and on Wada, in a debug build too, so the assertions run.
- Checksums are compared on one platform. The same code gives Wada `a6a04ff670d2c2de` on
  x86-64 and `1af4d0bd8806698d` on the tablet, because the maths libraries round differently.
  Kept worlds stay on the computer that built them, so that difference doesn't matter.
- A split that would change the output is its own `GENERATOR` change, in its own PR.

### Core residency (2b)

While riding, the core holds the whole parts (road, structures, railways, horizon), the chunk
index and a few recently read chunks.

- `world_chunk_for_season` inflates the chunk it is asked for from the store.
- `App::world()` no longer hands out all the chunks.
- The winter ground mask takes its bounds from the chunk index.
- The render tools and the benchmark read the chunks one by one, through the same call.

### Godot frees far chunks (2c)

- A chunk node, with its ground LODs, is freed once its centre is farther than the upload
  radius plus 240 m (`REORDER_DISTANCE`) plus one chunk.
- It is uploaded again when it comes back within the radius, within the same per-frame budget
  (R224).
- The radius is measured from the camera, so the free camera of simulated rides keeps its
  surroundings too.

### Generate ahead of the rider (2d, #237)

This supersedes the end of R224 ("course world data generation still finishes before scene
uploads").

- **Before riding.** The whole-route preparation runs first: the projection, the road index,
  ways, portals, the urban network and walking grades. Then come the chunks within the upload
  radius of the start, and the structures over them. The ride then starts. The rest is built in
  the background, ahead of the rider first, then behind.
- **Where the start is.** The start is where the rider enters the course, which may be partway
  along it, as when continuing a course (R220).
- **Group rides:**
  - In the lobby, **Ready** means the whole-route preparation is done and the surroundings of
    the group's start are finished.
  - A late joiner (R167, R171) prepares the same way. On **Join ride now**, the surroundings
    of the organizer's latest position, and the next 2 km ahead of it, come first. Entry waits
    until the chunks around the entry point, taken from the organizer's progress at that
    moment, are finished. If the organizer has moved past them, building follows them until
    they are.
  - A late joiner waits for finished chunks, while riders already in the group ride on
    without them. Entering is an explicit step that happens once, so a short wait costs less
    than starting in an empty scene. Riders already in the ride can't be asked to wait.
- **Throttle.** While a ride runs, background building starts at half the cores (at least
  one), at low priority, so drawing keeps its frame time and a tablet stays cool.
  - Its share rises, up to all cores, when its lead over the rider (or the group's front
    rider) falls below twice the upload radius. It falls back once the lead has doubled.
  - The 2d PR measures the throttled rate on the tablet **while riding**, in dense town chunks
    and on a fast descent (1.5 km a minute), not as a route average. The one figure so far,
    about 1.9 chunks a second (about 9 km of route a minute), came from all cores with nothing
    drawn, while the tablet was thrashing swap.
- **When the rider catches up** with chunks that aren't finished:
  - **Solo:** the ride pauses itself exactly as the pause button does (`App::set_paused`).
    Clock, recording and distance stop, and the trainer is released. The loading page shows.
    When the chunks are ready, the ride resumes by itself, and the grade is sent to the
    trainer again.
  - **Group ride:** nothing pauses, because the others keep riding. The road, structures and
    horizon are whole parts, so they are always there. The rider rides on them while the
    missing chunks are built first, and the chunks appear as they arrive. Whether the horizon's
    ground reaches under the corridor decides whether that looks like a road over a hole. The
    2d PR renders it.
  - **Jump** in a simulated ride (`App::jump_to`, `jump_near`, 2c too): the simulated ride
    pauses at the new point as above, clock included, until its surroundings are uploaded. A
    simulated ride is recorded, flagged as simulated, and sets no records, so nothing else
    changes.
- **Leaving early.** A ride left before its world is complete:
  - With **Keep courses ready to ride** on, building goes on in the background, throttled,
    while ORQWA stays open, also after the course is closed. The world is kept once complete,
    as R101 promises for a course first ridden.
  - When a ride of another course starts, or another world is to be built, the build is
    **stopped, not paused**. Its preparation and two rings are dropped from memory, so they
    never share memory with another ride, and R240's one-course figures still hold. Its
    store, with the chunks already written, stays: ORQWA holds on to it for the course's
    next build.
  - Afterwards, the build prepares the route again and continues with the chunks not yet
    written. A written chunk whose neighbours aren't finished is built again in memory for
    its ground and as-built pavement, but not written again. Building is deterministic, so
    the result is byte-identical; the cost is preparing twice.
  - If ORQWA closes first, the partial store is deleted by age, as above, and the next ride
    builds the world again.
  - Otherwise, the store is deleted with the course.

### Steps and measuring

The steps land as separate PRs off `main`, in this order, after #541. Each comes with
`world_memory` numbers from the container and, where it changes the device's picture, from
the tablet. Compact vertices (with the first `FORMAT` change) and coarser far streets (a
`GENERATOR` change, with renders) stay separate, as #542 lists. How each PR measures:

- **Compare peaks** (`VmHWM`), not resident memory after the build. glibc keeps much of what
  is freed, so resident memory falls less than what is given back. Freed chunks are reused for
  the next ones, so the peak is still bounded.
- **On Android, count swap as well:** `VmSwap`, or `dumpsys meminfo`. On the tablet, #541's
  build peaked at 2.8 GB resident but 4.4 GB counting zram, and the low-memory killer judges
  the whole.
- **Guard the tablet run.** A process started from `adb shell` can't be ended by the
  low-memory killer, and `main`'s build took the whole tablet down. The test stops the build
  when available memory falls below a threshold, or runs it under a memory limit.
- **R240's figures** are checked by the 2b PR (core while riding), the 2c PR (the app as a
  whole on the tablet) and the 2d PR (frame time and start).

## Consequences

- **The core's memory while riding** is the whole parts, the chunk index and a few chunks.
  **While building**, it is the whole-route preparation plus two rings of chunks around the
  build front. Only the whole parts, the index and the preparation grow with the route, and a
  densely mapped chunk is still bigger than a sparse one.
- **Godot's memory** follows the graphics preset's distance, so Low uses less, unlike today.
  Per chunk, the scene still costs what the chunk's meshes cost, until compact vertices.
- **Every rider now writes the world to the cache:** 10 to 25 MB per kilometre, about 1.1 GB
  for Wada, also without **Keep courses ready to ride**. Without that setting it is deleted
  when the course is closed, so on disk those worlds are temporary. On Android, a rider without
  that much free storage can't ride the course and is told why.
- **Reading back.** A chunk the rider returns to is read from disk instead of memory: a few
  milliseconds per chunk, within the upload budget.
- **Faster first rides.** After 2d, the first ride of a course starts once its surroundings
  are built, not the whole world. That includes courses imported as a file and every ride for
  riders who don't keep worlds. A kept world is read, as before.
- **Missing chunks mid-ride.** A solo rider who outruns the builder sees an automatic pause. A
  group rider sees chunks appear around the road.
- **Unreadable chunks.** A chunk that can't be read stays undrawn and is logged. That covers a
  damaged store and one Android cleared from the cache. A kept world is then marked damaged,
  so its next ride builds it again. Building the chunk again during the ride would mean keeping
  the whole-route preparation in memory.
