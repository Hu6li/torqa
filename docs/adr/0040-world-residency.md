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
  - protection heights look up the ground of the 3×3 chunks around them;
  - a barrier or fence is checked only against pavement in the cells it reaches;
  - a structure's foundations read the ground under its footprint.

  The ground mesh is final when its chunk is built. Later passes change only streets,
  tracks, structures and props.
- **Kept worlds** (ADR 0025) already store a zip of `whole` and `chunks/<n>`, with each chunk's
  centre as its first field. Riders who don't keep worlds have no store.

Options considered:

- **Keep chunks deflated in memory:** about 2.4 times smaller (1.3 GB on Wada), but memory
  still grows with the route's length.
- **Build far chunks again when needed:** keeps the map, elevations and preparation in memory,
  and a tablet builds 5 to 10 times slower than it reads, using the CPU it needs for drawing.
- **Store uncompressed:** saves CPU, but takes 3.2 GB of disk for Wada.
- **Thin the far world** (coarser far streets, compact vertices, no urban mesh far away): each
  is worth doing (#542), but each only scales the curve down, and two of them change the
  world.
- **Write each finished chunk to a store on disk and keep only what is drawn:** memory then
  depends on the surroundings and the preset, not on the course.

## Decision

Memory for building and riding a world depends on the rider's surroundings and the graphics
preset, not on the length of the course (R240). Every step leaves the built world
byte-identical, so neither `GENERATOR` nor `stored::FORMAT` is raised for this.

- **One store, always.** Every world is built into a store in the cache directory, never the
  data directory (R30):
  - While the world is being built, each finished chunk is written to
    `<cache>/worlds/<fingerprint>.building/chunks/<n>`, one single-entry zip per chunk, deflated
    at level 1 as kept worlds are. Each chunk can be read as soon as it is written, also while
    others are still being built.
  - When building completes and the rider keeps worlds, the chunks are packed into the kept
    `<fingerprint>.world` by raw copy (no inflating and deflating again), and the building
    directory is deleted. The kept world's layout stays as ADR 0025 describes it.
  - When the rider doesn't keep worlds, the building directory is the ride's store. It is
    deleted when the course is closed or another course is opened, and at start, after a crash
    or a low-memory kill. It costs about 10 MB of disk per kilometre while the course is open.
  - If the store can't be written (disk full, unwritable cache), the chunks stay in memory as
    they do today, with a warning in the log. Desktops ride as before.
- **Chunk index without a format change.** A chunk's centre is the first field of its entry.
  Reading a kept world therefore inflates only those 12 bytes per entry to index it. Chunk `n`
  keeps its index in the corridor's cell order (`chunks_near_route`), whatever order chunks are
  built in.
- **Generation in two stages (2a).** A chunk is *built* when `Corridor::chunk` returns. It is
  *finished* once its 3×3 neighbours are built and the structures over it stand. It is then
  handed to a sink and dropped from memory.
  - Protection heights, barriers against pavement and fences against pavement each run on the
    finished chunk's neighbourhood instead of on the whole world. Each chunk asks for its
    pavement cells as #541's `pavement::Asked` does for the whole world, so the
    `debug_assert` keeps catching a check that reaches beyond them.
  - `foundations::Ground` looks up chunks by cell instead of scanning them all.
  - Structures are built once the chunks under them are built. Ownership questions (earlier
    traversals, mapped decks, crossings) need only the road and the map, so they are settled
    before the chunk loop. `World::structures` stays a whole part.
  - Chunks are built in route order: a cell's rank is the first route distance that reaches
    it. Neighbours are then built close together, and few built chunks wait in memory for
    theirs. Cell order would hold a whole column of the corridor.
  - `generate()` stays as a sink that collects the full `World`, so the tests, the CLI and the
    checksums keep working on whole worlds. The gate for 2a is the same `world_memory`
    checksums as before the change, on the `kirchenfeldbruecke`, `bielersee` and `oberalp`
    fixtures and on Wada, in a debug build too, so the assertions run. The checksums are
    compared on one platform: the same code gives Wada `a6a04ff670d2c2de` on x86-64 and
    `1af4d0bd8806698d` on the tablet, because the maths libraries round differently. Kept
    worlds stay on the computer that built them, so that difference doesn't matter. A split
    that would change the output is its own `GENERATOR` change, in its own PR.
- **Core residency (2b).** While riding, the core holds the whole parts (road, structures,
  railways, horizon), the chunk index and a few recently read chunks. `world_chunk_for_season`
  inflates the chunk it is asked for from the store. `App::world()` no longer hands out all the
  chunks. The winter ground mask takes its bounds from the chunk index. The render tools and
  the benchmark read the chunks one by one, through the same call.
- **Godot frees far chunks (2c).** A chunk node, with its ground LODs, is freed once its centre
  is farther than the upload radius plus 240 m (`REORDER_DISTANCE`) plus one chunk. It is
  uploaded again when it comes back within the radius, within the same per-frame budget
  (R224). The radius is measured from the camera, so the free camera of simulated rides keeps
  its surroundings too. If the camera reaches chunks that aren't uploaded yet, as after a jump,
  the loading page is shown, as when a ride starts (R224).
- **Generate ahead of the rider (2d, #237).**
  - Everything for the whole route comes first: the projection, the road index, ways, portals,
    the urban network and walking grades.
  - Then come the chunks within the upload radius of the start, and the structures over them.
    The start is where the rider enters the course, which can be partway along it (continuing a
    course, joining a group ride).
  - The ride then starts. The remaining chunks are built in the background, ahead of the rider
    first, then behind.
  - While the ride runs, background building uses at most half the cores (at least one), at
    low priority, so drawing keeps its frame time and a tablet stays cool. It still keeps ahead
    of the rider. The tablet built Wada at about 1.9 chunks a second on all cores, about 9 km
    of route a minute, while a rider at 60 km/h covers 1 km a minute. That run was thrashing
    zram swap the whole time, so the rate is a lower bound.
  - If the rider reaches chunks that aren't finished, the loading page is shown until they
    are, as when a ride starts.
  - A ride left before its world is complete deletes the building directory. The next ride
    builds the world again.

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

## Consequences

- The core's memory while riding stops growing with the course: the whole parts, the chunk
  index and a few chunks. While building, it is the whole-route preparation plus the chunks
  waiting for neighbours.
- Godot's memory follows the graphics preset's distance, so Low uses less, unlike today. Per
  chunk, the scene still costs what the chunk's meshes cost, until compact vertices.
- Every rider now writes the world to the cache, about 10 MB per kilometre, also without
  **Keep courses ready to ride**. Without that setting, it is deleted when the course is
  closed. On disk, worlds of courses not kept are temporary.
- A chunk the rider returns to is read from disk instead of memory: a few milliseconds per
  chunk, within the upload budget.
- After 2d, the first ride of a course starts once its surroundings are built, not the whole
  world. That includes courses imported as a file and every ride for riders who don't keep
  worlds. A kept world is read, as before.
- The ride now reads from disk. A chunk that can't be read (a damaged store) stays undrawn and
  is logged, and a kept world is marked damaged so its next ride builds it again. Building it
  again during the ride would mean keeping the whole-route preparation in memory.
